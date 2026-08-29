using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Virtue.WindowsApp.Core.Interop;
using Virtue.WindowsApp.Core.ViewModels;
using Windows.Graphics;
using WinRT.Interop;

namespace Virtue.WindowsApp;

public sealed partial class MainWindow : Window
{
    private const string WebsiteDisplayUrl = "virtueinitiative.org";
    private const string WebsiteNavigateUrl = "https://virtueinitiative.org";
    private const string SignUpNavigateUrl = "https://app.virtueinitiative.org/signup";

    private readonly AppWindow _appWindow;
    private readonly TextBlock _statusTextBlock;
    private readonly TextBlock _statusDetailTextBlock;
    private readonly TextBlock _buildLabelTextBlock;
    private readonly TextBlock _updateCheckStatusTextBlock;
    private readonly TextBlock _accountSummaryTextBlock;
    private readonly StackPanel _loginPanel;
    private readonly StackPanel _accountActionsPanel;
    private readonly StackPanel _signedInActionsPanel;
    private readonly TextBox _emailTextBox;
    private readonly PasswordBox _passwordBox;
    private readonly TextBox _deviceNameTextBox;
    private readonly Border _statusDot;
    private readonly TextBlock _errorTextBlock;
    private readonly Border _updateNoticeCard;
    private readonly TextBlock _updateNoticeTextBlock;
    private readonly Button _restartNowButton = CreateActionButton("Restart now to update");
    private Button? _signInButton;
    private bool _allowClose;

    // Warm institutional palette (see shared-web/DESIGN-GUIDELINES.md).
    private static readonly SolidColorBrush PaperBrush = HexBrush("#F4EFE3");
    private static readonly SolidColorBrush SurfaceBrush = HexBrush("#FBF7EA");
    private static readonly SolidColorBrush PaperInsetBrush = HexBrush("#EBE4CE");
    private static readonly SolidColorBrush BorderBrushToken = HexBrush("#D9D1BC");
    private static readonly SolidColorBrush BorderHoverBrush = HexBrush("#C9C0A8");
    private static readonly SolidColorBrush InkBrush = HexBrush("#1B1A16");
    private static readonly SolidColorBrush Ink2Brush = HexBrush("#3A382F");
    private static readonly SolidColorBrush Ink3Brush = HexBrush("#6A6655");
    private static readonly SolidColorBrush ForestBrush = HexBrush("#1E3A2E");
    private static readonly SolidColorBrush Forest2Brush = HexBrush("#163026");
    private static readonly SolidColorBrush Forest3Brush = HexBrush("#2C4D3E");
    private static readonly SolidColorBrush SuccessBrush = HexBrush("#4F7A5A");
    private static readonly SolidColorBrush WarningBrush = HexBrush("#9C6B2E");
    private static readonly SolidColorBrush DangerBrush = HexBrush("#8B3A2A");

    // The design fonts ship via Google Fonts on the web; on Windows we fall back
    // to the nearest installed family in each comma-separated list.
    private static readonly FontFamily BodyFont = new("IBM Plex Sans, Segoe UI, sans-serif");
    private static readonly FontFamily MonoFont = new("IBM Plex Mono, Consolas, monospace");

    public MainWindow(SessionViewModel viewModel)
    {
        ViewModel = viewModel;
        Title = "Virtue";

        _statusTextBlock = new TextBlock();
        _statusDetailTextBlock = new TextBlock { TextWrapping = TextWrapping.Wrap };
        _buildLabelTextBlock = new TextBlock();
        _updateCheckStatusTextBlock = new TextBlock();
        _accountSummaryTextBlock = new TextBlock();
        _emailTextBox = new TextBox();
        _passwordBox = new PasswordBox();
        _deviceNameTextBox = new TextBox();
        _loginPanel = new StackPanel();
        _accountActionsPanel = new StackPanel();
        _signedInActionsPanel = new StackPanel();
        _statusDot = new Border
        {
            Width = 10,
            Height = 10,
            CornerRadius = new CornerRadius(2),
            VerticalAlignment = VerticalAlignment.Center,
        };
        _errorTextBlock = new TextBlock
        {
            TextWrapping = TextWrapping.Wrap,
            Foreground = DangerBrush,
            FontFamily = BodyFont,
        };
        _updateNoticeTextBlock = new TextBlock
        {
            TextWrapping = TextWrapping.Wrap,
            FontFamily = BodyFont,
            Foreground = InkBrush,
        };
        _updateNoticeCard = BuildUpdateNoticeCard();

        Content = BuildContent();
        ViewModel.PropertyChanged += ViewModelOnPropertyChanged;
        _appWindow = ResolveAppWindow();
        _appWindow.Closing += AppWindowOnClosing;
        _appWindow.Resize(new SizeInt32(720, 560));
        SetWindowIcon();
        SyncFromViewModel();
        IsVisibleToUser = true;
    }

    public SessionViewModel ViewModel { get; }

    public bool IsVisibleToUser { get; private set; }

    /// <summary>
    /// Raised at the end of <see cref="HideToTray"/>, i.e. every time the window transitions
    /// to hidden — the X-button close, the tray-Exit confirmation's cancel-path re-hide, and
    /// the update flow's own hide all funnel through here uniformly. Lets
    /// <c>App.EvaluateUpdateRestart</c> react the instant the window closes instead of waiting
    /// for the next countdown tick.
    /// </summary>
    public event EventHandler? Hidden;

    /// <summary>Raised by the in-window update notice's "Restart now to update" button.</summary>
    public event EventHandler? CloseNowAndUpdateRequested;

    /// <summary>
    /// Raised by the status card's "Check for updates" button. Its only effect is to
    /// short-circuit the update check's 4-hour cadence — see
    /// <c>StoreUpdateManager.RequestCheckNow</c>.
    /// </summary>
    public event EventHandler? CheckForUpdatesRequested;

    public void HideToTray()
    {
        var windowHandle = WindowNative.GetWindowHandle(this);
        ShowWindow(windowHandle, ShowWindowCommands.Hide);
        IsVisibleToUser = false;
        Hidden?.Invoke(this, EventArgs.Empty);
    }

    public void ShowFromTray()
    {
        var windowHandle = WindowNative.GetWindowHandle(this);
        ShowWindow(windowHandle, ShowWindowCommands.Restore);
        Activate();
        SetForegroundWindow(windowHandle);
        IsVisibleToUser = true;
    }

    public void PrepareForExit()
    {
        _allowClose = true;
    }

    private UIElement BuildContent()
    {
        _emailTextBox.PlaceholderText = "Email";
        _emailTextBox.TextChanged += (_, _) => ViewModel.EmailInput = _emailTextBox.Text;
        StyleInput(_emailTextBox);

        _passwordBox.PlaceholderText = "Password";
        // `Peek` is meant to draw WinUI's own reveal button inside the box, but
        // it never renders here, so the reveal is driven from our own toggle
        // (see BuildPasswordRow) flipping this between Hidden and Visible.
        _passwordBox.PasswordRevealMode = PasswordRevealMode.Hidden;
        _passwordBox.PasswordChanged += PasswordBox_OnPasswordChanged;
        StyleInput(_passwordBox);

        _deviceNameTextBox.PlaceholderText = "Device name";
        _deviceNameTextBox.Text = ViewModel.DeviceNameInput;
        _deviceNameTextBox.TextChanged += (_, _) => ViewModel.DeviceNameInput = _deviceNameTextBox.Text;
        StyleInput(_deviceNameTextBox);

        var root = new Grid
        {
            Background = PaperBrush,
        };

        var contentStack = new StackPanel
        {
            Spacing = 20,
            Padding = new Thickness(28, 30, 28, 28),
        };

        contentStack.Children.Add(BuildHeader());
        contentStack.Children.Add(_updateNoticeCard);
        contentStack.Children.Add(BuildStatusCard());
        contentStack.Children.Add(BuildAccountCard());

        root.Children.Add(new ScrollViewer
        {
            Content = contentStack,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
        });

        return root;
    }

    private UIElement BuildHeader()
    {
        var header = new Grid();
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        var icon = new Image
        {
            Width = 60,
            Height = 60,
            Margin = new Thickness(0, 0, 16, 0),
            Source = new BitmapImage { UriSource = new Uri("ms-appx:///Assets/app-icon.png") },
        };
        Grid.SetColumn(icon, 0);
        header.Children.Add(icon);

        var textStack = new StackPanel { Spacing = 4 };
        Grid.SetColumn(textStack, 1);
        header.Children.Add(textStack);

        textStack.Children.Add(new TextBlock
        {
            Text = "Virtue",
            FontFamily = BodyFont,
            FontSize = 32,
            FontWeight = FontWeights.SemiBold,
            Foreground = InkBrush,
        });

        var websiteLink = new HyperlinkButton
        {
            Content = WebsiteDisplayUrl,
            NavigateUri = new Uri(WebsiteNavigateUrl),
            HorizontalAlignment = HorizontalAlignment.Left,
            Padding = new Thickness(0),
            FontFamily = MonoFont,
            Foreground = ForestBrush,
        };
        websiteLink.Resources["HyperlinkButtonForeground"] = ForestBrush;
        websiteLink.Resources["HyperlinkButtonForegroundPointerOver"] = Forest3Brush;
        websiteLink.Resources["HyperlinkButtonForegroundPressed"] = Forest2Brush;
        websiteLink.Resources["HyperlinkButtonForegroundDisabled"] = Ink3Brush;
        textStack.Children.Add(websiteLink);

        return header;
    }

    private Border BuildUpdateNoticeCard()
    {
        _restartNowButton.Click += (_, _) => CloseNowAndUpdateRequested?.Invoke(this, EventArgs.Empty);

        var content = new StackPanel { Spacing = 8 };
        content.Children.Add(CreateSectionLabel("Update Ready"));
        content.Children.Add(_updateNoticeTextBlock);
        content.Children.Add(_restartNowButton);

        var card = CreateCard(content);
        card.BorderBrush = WarningBrush;
        return card;
    }

    private UIElement BuildStatusCard()
    {
        _statusTextBlock.FontFamily = BodyFont;
        _statusTextBlock.FontSize = 24;
        _statusTextBlock.FontWeight = FontWeights.SemiBold;
        _statusTextBlock.Foreground = InkBrush;
        _statusTextBlock.VerticalAlignment = VerticalAlignment.Center;

        _statusDetailTextBlock.FontFamily = BodyFont;
        _statusDetailTextBlock.Foreground = Ink2Brush;
        _buildLabelTextBlock.FontFamily = MonoFont;
        _buildLabelTextBlock.FontSize = 12;
        _buildLabelTextBlock.Foreground = Ink3Brush;
        _updateCheckStatusTextBlock.FontFamily = BodyFont;
        _updateCheckStatusTextBlock.FontSize = 12;
        _updateCheckStatusTextBlock.Foreground = Ink3Brush;
        _updateCheckStatusTextBlock.VerticalAlignment = VerticalAlignment.Center;

        var headingRow = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 10,
        };
        headingRow.Children.Add(_statusDot);
        headingRow.Children.Add(_statusTextBlock);

        var detailsButton = CreateActionButton("Status Details");
        detailsButton.Click += StatusDetailsButton_OnClick;

        var reportBugButton = CreateActionButton("Report a Bug");
        reportBugButton.Click += async (_, _) => await ShowReportBugDialogAsync();

        var forceCaptureButton = CreateActionButton("Test Screenshot");
        forceCaptureButton.Click += async (_, _) => await ForceCaptureButton_OnClickAsync();

        var stopMonitoringButton = CreateActionButton("Stop Monitoring");
        stopMonitoringButton.Click += StopMonitoringButton_OnClick;

        var checkForUpdatesButton = CreateActionButton("Check for updates");
        checkForUpdatesButton.Click += (_, _) => CheckForUpdatesRequested?.Invoke(this, EventArgs.Empty);

        _signedInActionsPanel.Orientation = Orientation.Horizontal;
        _signedInActionsPanel.Spacing = 10;
        _signedInActionsPanel.Children.Add(forceCaptureButton);
        _signedInActionsPanel.Children.Add(stopMonitoringButton);

        var actionRow = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 10,
            Margin = new Thickness(0, 18, 0, 0),
        };
        actionRow.Children.Add(detailsButton);
        actionRow.Children.Add(reportBugButton);
        actionRow.Children.Add(_signedInActionsPanel);

        var buildRow = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 10,
            Margin = new Thickness(0, 6, 0, 0),
        };
        buildRow.Children.Add(checkForUpdatesButton);
        buildRow.Children.Add(_updateCheckStatusTextBlock);

        var content = new StackPanel { Spacing = 8 };
        content.Children.Add(CreateSectionLabel("Status"));
        content.Children.Add(headingRow);
        content.Children.Add(_statusDetailTextBlock);
        content.Children.Add(_buildLabelTextBlock);
        content.Children.Add(buildRow);
        content.Children.Add(actionRow);

        return CreateCard(content);
    }

    private UIElement BuildAccountCard()
    {
        _accountSummaryTextBlock.FontFamily = BodyFont;
        _accountSummaryTextBlock.FontSize = 18;
        _accountSummaryTextBlock.Foreground = InkBrush;

        _loginPanel.Spacing = 12;
        _loginPanel.Margin = new Thickness(0, 12, 0, 0);
        _loginPanel.Children.Add(_emailTextBox);
        _loginPanel.Children.Add(BuildPasswordRow());
        _loginPanel.Children.Add(_deviceNameTextBox);

        _signInButton = CreatePrimaryButton("Sign In");
        _signInButton.Click += SignInButton_OnClick;
        _loginPanel.Children.Add(_signInButton);

        var signOutButton = CreateActionButton("Sign Out");
        signOutButton.Click += async (_, _) =>
        {
            if (await ShowLogoutConfirmationAsync())
            {
                await ViewModel.LogoutAsync();
            }
        };

        var signUpLink = new HyperlinkButton
        {
            Content = "Don't have an account? Sign up",
            NavigateUri = new Uri(SignUpNavigateUrl),
            HorizontalAlignment = HorizontalAlignment.Left,
            Padding = new Thickness(0),
            FontFamily = BodyFont,
            Foreground = ForestBrush,
        };
        signUpLink.Resources["HyperlinkButtonForeground"] = ForestBrush;
        signUpLink.Resources["HyperlinkButtonForegroundPointerOver"] = Forest3Brush;
        signUpLink.Resources["HyperlinkButtonForegroundPressed"] = Forest2Brush;
        signUpLink.Resources["HyperlinkButtonForegroundDisabled"] = Ink3Brush;
        _loginPanel.Children.Add(signUpLink);

        _accountActionsPanel.Orientation = Orientation.Horizontal;
        _accountActionsPanel.Spacing = 10;
        _accountActionsPanel.Margin = new Thickness(0, 12, 0, 0);
        _accountActionsPanel.Children.Add(signOutButton);

        var content = new StackPanel { Spacing = 8 };
        content.Children.Add(CreateSectionLabel("Account"));
        content.Children.Add(_accountSummaryTextBlock);
        content.Children.Add(_loginPanel);
        content.Children.Add(_accountActionsPanel);
        content.Children.Add(_errorTextBlock);

        return CreateCard(content);
    }

    private static Border CreateCard(UIElement content) =>
        new()
        {
            Background = SurfaceBrush,
            CornerRadius = new CornerRadius(4),
            BorderBrush = BorderBrushToken,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(20),
            Child = content,
        };

    // Mono "eyebrow" label: uppercase, letter-spaced, muted — the design's
    // "stamped" small-caps feel for section headers and metadata.
    private static TextBlock CreateSectionLabel(string text) =>
        new()
        {
            Text = text.ToUpperInvariant(),
            FontFamily = MonoFont,
            FontSize = 12,
            FontWeight = FontWeights.Medium,
            CharacterSpacing = 80,
            Foreground = Ink3Brush,
        };

    // Filled-forest primary action with paper text (Button --primary).
    private static Button CreatePrimaryButton(string text)
    {
        var button = new Button
        {
            Content = text,
            HorizontalAlignment = HorizontalAlignment.Left,
            MinWidth = 120,
            FontFamily = BodyFont,
            CornerRadius = new CornerRadius(2),
            BorderThickness = new Thickness(1),
        };
        button.Resources["ButtonBackground"] = ForestBrush;
        button.Resources["ButtonBackgroundPointerOver"] = Forest3Brush;
        button.Resources["ButtonBackgroundPressed"] = Forest2Brush;
        button.Resources["ButtonForeground"] = PaperBrush;
        button.Resources["ButtonForegroundPointerOver"] = PaperBrush;
        button.Resources["ButtonForegroundPressed"] = PaperBrush;
        button.Resources["ButtonBorderBrush"] = ForestBrush;
        button.Resources["ButtonBorderBrushPointerOver"] = Forest3Brush;
        button.Resources["ButtonBorderBrushPressed"] = Forest2Brush;
        return button;
    }

    // Low-emphasis neutral action: subtle paper fill on a hairline border
    // (Button --quiet).
    private static Button CreateActionButton(string text)
    {
        var button = new Button
        {
            Content = text,
            HorizontalAlignment = HorizontalAlignment.Left,
            FontFamily = BodyFont,
            CornerRadius = new CornerRadius(2),
            BorderThickness = new Thickness(1),
        };
        button.Resources["ButtonBackground"] = SurfaceBrush;
        button.Resources["ButtonBackgroundPointerOver"] = PaperInsetBrush;
        button.Resources["ButtonBackgroundPressed"] = PaperInsetBrush;
        button.Resources["ButtonForeground"] = Ink2Brush;
        button.Resources["ButtonForegroundPointerOver"] = InkBrush;
        button.Resources["ButtonForegroundPressed"] = InkBrush;
        button.Resources["ButtonBorderBrush"] = BorderBrushToken;
        button.Resources["ButtonBorderBrushPointerOver"] = BorderHoverBrush;
        button.Resources["ButtonBorderBrushPressed"] = BorderHoverBrush;
        return button;
    }

    /// The password box plus the eye toggle that reveals what was typed, laid
    /// out so the box takes the remaining width and the button sits beside it.
    private Grid BuildPasswordRow()
    {
        var row = new Grid();
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        // The box stretches to the reveal button's height, which is taller than
        // a single line of text; without this the password sits pinned to the
        // top of the box while every other field's text is centered.
        _passwordBox.VerticalContentAlignment = VerticalAlignment.Center;

        Grid.SetColumn(_passwordBox, 0);
        row.Children.Add(_passwordBox);

        var revealIcon = new FontIcon
        {
            Glyph = "\uE7B3",
            FontSize = 16,
            Foreground = Ink2Brush,
        };
        var revealButton = new Button
        {
            Content = revealIcon,
            Margin = new Thickness(8, 0, 0, 0),
            Padding = new Thickness(10, 8, 10, 8),
            CornerRadius = new CornerRadius(2),
            BorderBrush = BorderBrushToken,
            Background = PaperBrush,
        };
        AutomationProperties.SetName(revealButton, "Show password");
        ToolTipService.SetToolTip(revealButton, "Show password");
        revealButton.Click += (_, _) =>
        {
            var reveal = _passwordBox.PasswordRevealMode != PasswordRevealMode.Visible;
            _passwordBox.PasswordRevealMode =
                reveal ? PasswordRevealMode.Visible : PasswordRevealMode.Hidden;
            revealIcon.Glyph = reveal ? "\uED1A" : "\uE7B3";
            var label = reveal ? "Hide password" : "Show password";
            AutomationProperties.SetName(revealButton, label);
            ToolTipService.SetToolTip(revealButton, label);
        };

        Grid.SetColumn(revealButton, 1);
        row.Children.Add(revealButton);
        return row;
    }

    // Cream-filled input on a hairline border; focus border goes forest.
    private static void StyleInput(Control input)
    {
        input.FontFamily = BodyFont;
        input.CornerRadius = new CornerRadius(2);
        input.Resources["TextControlBackground"] = PaperBrush;
        input.Resources["TextControlBackgroundPointerOver"] = PaperBrush;
        input.Resources["TextControlBackgroundFocused"] = SurfaceBrush;
        input.Resources["TextControlBorderBrush"] = BorderBrushToken;
        input.Resources["TextControlBorderBrushPointerOver"] = BorderHoverBrush;
        input.Resources["TextControlBorderBrushFocused"] = ForestBrush;
        input.Resources["TextControlForeground"] = InkBrush;
        input.Resources["TextControlForegroundPointerOver"] = InkBrush;
        input.Resources["TextControlForegroundFocused"] = InkBrush;
        input.Resources["TextControlPlaceholderForeground"] = Ink3Brush;
        input.Resources["TextControlPlaceholderForegroundPointerOver"] = Ink3Brush;
        input.Resources["TextControlPlaceholderForegroundFocused"] = Ink3Brush;
    }

    private async void StatusDetailsButton_OnClick(object sender, RoutedEventArgs e)
    {
        await ViewModel.RefreshAsync();
        await ShowStatusDialogAsync();
    }

    public async Task<bool> ShowStopMonitoringConfirmationAsync()
    {
        var warningIcon = new FontIcon
        {
            Glyph = "\uE7BA",
            FontSize = 28,
            Foreground = WarningBrush,
            Margin = new Thickness(0, 2, 16, 0),
        };

        var textStack = new StackPanel
        {
            Spacing = 8,
        };
        textStack.Children.Add(new TextBlock
        {
            Text = "Stop monitoring and close Virtue?",
            FontFamily = BodyFont,
            FontSize = 20,
            FontWeight = FontWeights.SemiBold,
            Foreground = InkBrush,
            TextWrapping = TextWrapping.Wrap,
        });
        textStack.Children.Add(new TextBlock
        {
            Text = "This will stop monitoring on this device and close the main window and tray app.",
            FontFamily = BodyFont,
            TextWrapping = TextWrapping.Wrap,
            Foreground = Ink2Brush,
        });
        textStack.Children.Add(new TextBlock
        {
            Text = "People monitoring you may be alerted.",
            FontFamily = BodyFont,
            TextWrapping = TextWrapping.Wrap,
            Foreground = WarningBrush,
            FontWeight = FontWeights.Medium,
        });

        var content = new Grid
        {
            ColumnSpacing = 0,
            MinWidth = 420,
        };
        content.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        content.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(warningIcon, 0);
        Grid.SetColumn(textStack, 1);
        content.Children.Add(warningIcon);
        content.Children.Add(textStack);

        var dialog = new ContentDialog
        {
            Title = CreateDialogTitle("Stop Monitoring"),
            PrimaryButtonText = "Stop Monitoring",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            Content = content,
        };
        ApplyDialogTheme(dialog);

        if (Content is FrameworkElement root)
        {
            dialog.XamlRoot = root.XamlRoot;
        }

        var result = await dialog.ShowAsync();
        return result == ContentDialogResult.Primary;
    }

    private async Task<bool> ShowLogoutConfirmationAsync()
    {
        var warningIcon = new FontIcon
        {
            Glyph = "",
            FontSize = 28,
            Foreground = WarningBrush,
            Margin = new Thickness(0, 2, 16, 0),
        };

        // The dialog's own title carries the question, so the body starts
        // straight in on the consequences — a second heading inside the
        // content just repeats the chrome above it.
        var textStack = new StackPanel
        {
            Spacing = 8,
        };
        textStack.Children.Add(new TextBlock
        {
            Text = "Signing out will deactivate this device and stop monitoring. Logging in again will create a new device.",
            FontFamily = BodyFont,
            TextWrapping = TextWrapping.Wrap,
            Foreground = Ink2Brush,
        });
        textStack.Children.Add(new TextBlock
        {
            Text = "Anyone monitoring you may be alerted.",
            FontFamily = BodyFont,
            TextWrapping = TextWrapping.Wrap,
            Foreground = WarningBrush,
            FontWeight = FontWeights.Medium,
        });

        var content = new Grid
        {
            ColumnSpacing = 0,
            MinWidth = 420,
        };
        content.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        content.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(warningIcon, 0);
        Grid.SetColumn(textStack, 1);
        content.Children.Add(warningIcon);
        content.Children.Add(textStack);

        var dialog = new ContentDialog
        {
            Title = CreateDialogTitle("Sign out of Virtue?"),
            PrimaryButtonText = "Sign Out",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            Content = content,
        };
        ApplyDialogTheme(dialog);

        if (Content is FrameworkElement root)
        {
            dialog.XamlRoot = root.XamlRoot;
        }

        var result = await dialog.ShowAsync();
        return result == ContentDialogResult.Primary;
    }

    private async Task ShowStatusDialogAsync()
    {
        var statusBlock = new TextBlock
        {
            Text = BuildStatusDetailsText(),
            TextWrapping = TextWrapping.Wrap,
            FontFamily = MonoFont,
            FontSize = 13,
            Foreground = Ink2Brush,
            Width = 520,
        };

        var logDirectory = ViewModel.MonitorStatus?.LogDirectory;
        var dialog = new ContentDialog
        {
            Title = CreateDialogTitle("Virtue Status"),
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
            Content = new ScrollViewer
            {
                Content = statusBlock,
                VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
                HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
                MinHeight = 320,
            },
        };

        if (!string.IsNullOrWhiteSpace(logDirectory))
        {
            dialog.PrimaryButtonText = "Open log folder";
            dialog.PrimaryButtonClick += (_, args) =>
            {
                // Keep the dialog open: opening Explorer isn't a "done here"
                // action, and closing would force a reopen to read on.
                args.Cancel = true;
                OpenLogFolder(logDirectory!);
            };
        }
        ApplyDialogTheme(dialog);

        if (Content is FrameworkElement root)
        {
            dialog.XamlRoot = root.XamlRoot;
        }

        await dialog.ShowAsync();
    }

    public async Task ShowReportBugDialogAsync()
    {
        var messageBox = new TextBox
        {
            PlaceholderText = "Describe the issue",
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            Height = 120,
        };
        StyleInput(messageBox);

        var contactEmailBox = new TextBox
        {
            PlaceholderText = "Contact email (optional)",
            Text = ViewModel.LoggedIn ? ViewModel.AccountEmail : string.Empty,
        };
        StyleInput(contactEmailBox);

        var includeLogsCheckBox = new CheckBox
        {
            Content = "Include the last day of diagnostic logs",
            IsChecked = true,
            FontFamily = BodyFont,
            Foreground = InkBrush,
        };

        var includeLogsCaption = new TextBlock
        {
            Text = "Includes timestamps, monitoring status, and error messages from the last day. " +
                   "No screenshots or window titles are included. Known tokens are redacted automatically.",
            TextWrapping = TextWrapping.Wrap,
            FontFamily = BodyFont,
            FontSize = 12,
            Foreground = Ink3Brush,
            Margin = new Thickness(28, 0, 0, 0),
        };

        var reportErrorTextBlock = new TextBlock
        {
            TextWrapping = TextWrapping.Wrap,
            Foreground = DangerBrush,
            FontFamily = BodyFont,
            Visibility = Visibility.Collapsed,
        };

        var content = new StackPanel { Spacing = 12, Width = 420 };
        content.Children.Add(messageBox);
        content.Children.Add(contactEmailBox);
        content.Children.Add(includeLogsCheckBox);
        content.Children.Add(includeLogsCaption);
        content.Children.Add(reportErrorTextBlock);

        var dialog = new ContentDialog
        {
            Title = CreateDialogTitle("Report a Bug"),
            PrimaryButtonText = "Send Report",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
            Content = content,
        };
        ApplyDialogTheme(dialog);

        if (Content is FrameworkElement root)
        {
            dialog.XamlRoot = root.XamlRoot;
        }

        var reportSent = false;
        dialog.PrimaryButtonClick += async (_, args) =>
        {
            var message = messageBox.Text.Trim();
            if (string.IsNullOrEmpty(message))
            {
                args.Cancel = true;
                reportErrorTextBlock.Text = "Please describe the issue.";
                reportErrorTextBlock.Visibility = Visibility.Visible;
                return;
            }

            var deferral = args.GetDeferral();
            try
            {
                var contactEmail = contactEmailBox.Text.Trim();
                var succeeded = await ViewModel.SubmitBugReportAsync(
                    message,
                    string.IsNullOrEmpty(contactEmail) ? null : contactEmail,
                    includeLogsCheckBox.IsChecked == true);

                if (!succeeded)
                {
                    args.Cancel = true;
                    reportErrorTextBlock.Text = ViewModel.ErrorText ?? "Failed to send the report.";
                    reportErrorTextBlock.Visibility = Visibility.Visible;
                }
                else
                {
                    reportSent = true;
                }
            }
            finally
            {
                deferral.Complete();
            }
        };

        await dialog.ShowAsync();

        if (reportSent)
        {
            await ShowReportBugConfirmationAsync();
        }
    }

    private async Task ShowReportBugConfirmationAsync()
    {
        var dialog = new ContentDialog
        {
            Title = CreateDialogTitle("Report Sent"),
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
            Content = new TextBlock
            {
                Text = "Thanks — your report was sent to the Virtue Initiative team.",
                TextWrapping = TextWrapping.Wrap,
                FontFamily = BodyFont,
                Foreground = Ink2Brush,
                Width = 380,
            },
        };
        ApplyDialogTheme(dialog);

        if (Content is FrameworkElement root)
        {
            dialog.XamlRoot = root.XamlRoot;
        }

        await dialog.ShowAsync();
    }

    /// <summary>
    /// The status page (client/core/SPEC.md CORE-010): the same sections, in
    /// the same order, as every other platform's status screen, plus the
    /// Windows-specific monitor state, package version, and log directory.
    /// </summary>
    private string BuildStatusDetailsText()
    {
        var status = ViewModel.MonitorStatus;
        var lines = new List<string>
        {
            "Account",
            $"  signed in:          {ViewModel.LoggedIn.ToString().ToLowerInvariant()}",
            $"  email:              {DisplayOrPlaceholder(ViewModel.AccountEmail)}",
            $"  device name:        {DisplayOrPlaceholder(status?.DeviceName)}",
            $"  partners:           {status?.PartnerCount?.ToString() ?? "<unknown>"}",
            string.Empty,
            "Queues",
            $"  waiting for hash:   {status?.PendingHashCount ?? 0}",
            $"  waiting in batch:   {status?.PendingBatchCount ?? 0}",
            $"  last batch upload:  {FormatUnixTimestamp(status?.LastBatchAtMs)}",
            string.Empty,
            "Capture",
            $"  monitor state:      {ViewModel.MonitorState}",
            $"  last loop:          {FormatUnixTimestamp(status?.LastLoopAtMs)}",
            $"  last attempt:       {FormatUnixTimestamp(status?.LastScreenshotAttemptAtMs)}",
            $"  last screenshot:    {FormatUnixTimestamp(ViewModel.LastScreenshotAtMs)}",
            $"  last skip reason:   {DisplayOrPlaceholder(status?.LastSkipReason)}",
            string.Empty,
            "Recent errors",
        };

        var recentErrors = status?.RecentErrors;
        if (recentErrors is null || recentErrors.Count == 0)
        {
            lines.Add(string.IsNullOrWhiteSpace(ViewModel.MonitorError)
                ? "  (none)"
                : $"  {ViewModel.MonitorError}");
        }
        else
        {
            foreach (var error in recentErrors.Take(5))
            {
                lines.Add($"  {FormatUnixTimestamp(error.AtMs)} [{error.Context}] {error.Message}");
            }
        }

        lines.Add(string.Empty);
        lines.Add("Advanced");
        lines.Add($"  device id:          {DisplayOrPlaceholder(ViewModel.DeviceId)}");
        lines.Add($"  api url:            {DisplayOrPlaceholder(status?.ApiBaseUrl)}");
        lines.Add($"  hash base url:      {(string.IsNullOrWhiteSpace(status?.HashBaseUrl) ? "<default>" : status!.HashBaseUrl)}");
        lines.Add($"  capture interval:   {(status?.CaptureIntervalSeconds is long capture ? $"{capture}s" : "<unknown>")}");
        lines.Add($"  batch window:       {(status?.BatchWindowSeconds is long batch ? $"{batch}s" : "<unknown>")}");
        lines.Add($"  build:              {ViewModel.BuildLabel}");

        if (!string.IsNullOrWhiteSpace(ViewModel.WindowsPackageVersion))
        {
            lines.Add($"  windows package:    {ViewModel.WindowsPackageVersion}");
        }

        lines.Add($"  logs:               {DisplayOrPlaceholder(status?.LogDirectory)}");

        return string.Join(Environment.NewLine, lines);
    }

    private async void SignInButton_OnClick(object sender, RoutedEventArgs e)
    {
        await ViewModel.LoginAsync();
    }

    private async void StopMonitoringButton_OnClick(object sender, RoutedEventArgs e)
    {
        if (Application.Current is App app)
        {
            await app.RequestResidentShutdownAsync();
        }
    }

    private async Task ForceCaptureButton_OnClickAsync()
    {
        var result = await ViewModel.ForceCaptureAsync();
        if (result is not null)
        {
            // The interop call waited for the batch, so `result.Message` says
            // what really happened: uploaded, gated, or still in flight.
            await ShowForceCaptureConfirmationAsync(result);
        }
    }

    private async Task ShowForceCaptureConfirmationAsync(ForceCapturePayload result)
    {
        var dialog = new ContentDialog
        {
            Title = CreateDialogTitle(result.Outcome == "uploaded" ? "Screenshot Uploaded" : "Test Screenshot"),
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
            Content = new TextBlock
            {
                Text = result.Message,
                TextWrapping = TextWrapping.Wrap,
                FontFamily = BodyFont,
                Foreground = Ink2Brush,
                Width = 380,
            },
        };
        ApplyDialogTheme(dialog);

        if (Content is FrameworkElement root)
        {
            dialog.XamlRoot = root.XamlRoot;
        }

        await dialog.ShowAsync();
    }

    private void PasswordBox_OnPasswordChanged(object sender, RoutedEventArgs e)
    {
        if (sender is PasswordBox passwordBox)
        {
            ViewModel.PasswordInput = passwordBox.Password;
        }
    }

    private void ViewModelOnPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        SyncFromViewModel();
    }

    private void SyncFromViewModel()
    {
        _updateNoticeCard.Visibility = ViewModel.UpdateReady ? Visibility.Visible : Visibility.Collapsed;
        _updateNoticeTextBlock.Text = ViewModel.UpdateInstalling
            ? "Installing the update. Virtue will close and restart itself within a minute."
            : ViewModel.UpdateCountdownText is { } countdown
                ? $"Virtue will restart to update in {countdown}."
                : "An update is ready and will install soon.";
        _restartNowButton.IsEnabled = !ViewModel.UpdateInstalling;

        _statusTextBlock.Text = BuildPrimaryStatusText();
        var statusBrush = StatusBrush();
        _statusTextBlock.Foreground = statusBrush;
        _statusDot.Background = statusBrush;
        _statusDetailTextBlock.Text = BuildSecondaryStatusText();
        _buildLabelTextBlock.Text = ViewModel.BuildLabelText;
        _updateCheckStatusTextBlock.Text = ViewModel.UpdateCheckStatusText ?? string.Empty;

        if (!ViewModel.HasLoadedStatus)
        {
            _accountSummaryTextBlock.Text = "Checking sign-in status...";
            _loginPanel.Visibility = Visibility.Collapsed;
            _accountActionsPanel.Visibility = Visibility.Collapsed;
            _signedInActionsPanel.Visibility = Visibility.Collapsed;
        }
        else
        {
            _accountSummaryTextBlock.Text = ViewModel.LoggedIn
                ? $"Signed in as {ViewModel.AccountSummary}"
                : "Sign in to start monitoring.";
            _loginPanel.Visibility = ViewModel.LoggedIn ? Visibility.Collapsed : Visibility.Visible;
            _accountActionsPanel.Visibility = ViewModel.LoggedIn ? Visibility.Visible : Visibility.Collapsed;
            _signedInActionsPanel.Visibility = ViewModel.LoggedIn ? Visibility.Visible : Visibility.Collapsed;
        }

        var loginEnabled = !ViewModel.IsBusy;
        _emailTextBox.IsEnabled = loginEnabled;
        _passwordBox.IsEnabled = loginEnabled;
        _deviceNameTextBox.IsEnabled = loginEnabled;
        if (_signInButton is not null) _signInButton.IsEnabled = loginEnabled;

        _errorTextBlock.Text = ViewModel.ErrorText ?? "";
        _errorTextBlock.Visibility = string.IsNullOrWhiteSpace(ViewModel.ErrorText)
            ? Visibility.Collapsed
            : Visibility.Visible;

        if (_emailTextBox.Text != ViewModel.EmailInput)
        {
            _emailTextBox.Text = ViewModel.EmailInput;
        }

        if (_passwordBox.Password != ViewModel.PasswordInput)
        {
            _passwordBox.Password = ViewModel.PasswordInput;
        }

        if (_deviceNameTextBox.Text != ViewModel.DeviceNameInput)
        {
            _deviceNameTextBox.Text = ViewModel.DeviceNameInput;
        }
    }

    private string BuildPrimaryStatusText() =>
        ViewModel.MonitorState switch
        {
            "loading" => "Loading status...",
            "running" => "Monitoring active",
            "starting" => "Starting monitoring",
            "signed_out" => "Logged out",
            "error" => "Attention needed",
            _ => "Monitoring stopped",
        };

    private SolidColorBrush StatusBrush() =>
        ViewModel.MonitorState switch
        {
            "running" => SuccessBrush,
            "starting" => ForestBrush,
            "error" => WarningBrush,
            _ => Ink3Brush,
        };

    private static TextBlock CreateDialogTitle(string text) =>
        new()
        {
            Text = text,
            FontFamily = BodyFont,
            FontWeight = FontWeights.SemiBold,
            Foreground = InkBrush,
        };

    // Lightweight styling so dialogs read as cream paper with forest/quiet
    // buttons and near-square corners, matching the rest of the page.
    private static void ApplyDialogTheme(ContentDialog dialog)
    {
        var r = dialog.Resources;
        r["ContentDialogBackground"] = SurfaceBrush;
        r["ContentDialogForeground"] = InkBrush;
        r["ContentDialogBorderBrush"] = BorderBrushToken;
        r["ContentDialogSeparatorBorderBrush"] = BorderBrushToken;
        r["OverlayCornerRadius"] = new CornerRadius(4);
        r["ControlCornerRadius"] = new CornerRadius(2);

        // Default button (accent) -> filled forest.
        r["AccentButtonBackground"] = ForestBrush;
        r["AccentButtonBackgroundPointerOver"] = Forest3Brush;
        r["AccentButtonBackgroundPressed"] = Forest2Brush;
        r["AccentButtonForeground"] = PaperBrush;
        r["AccentButtonForegroundPointerOver"] = PaperBrush;
        r["AccentButtonForegroundPressed"] = PaperBrush;
        r["AccentButtonBorderBrush"] = ForestBrush;

        // Non-default buttons -> quiet paper fill on a hairline border.
        r["ButtonBackground"] = SurfaceBrush;
        r["ButtonBackgroundPointerOver"] = PaperInsetBrush;
        r["ButtonBackgroundPressed"] = PaperInsetBrush;
        r["ButtonForeground"] = Ink2Brush;
        r["ButtonForegroundPointerOver"] = InkBrush;
        r["ButtonForegroundPressed"] = InkBrush;
        r["ButtonBorderBrush"] = BorderBrushToken;
        r["ButtonBorderBrushPointerOver"] = BorderHoverBrush;
        r["ButtonBorderBrushPressed"] = BorderHoverBrush;
    }

    private string BuildSecondaryStatusText()
    {
        if (!string.IsNullOrWhiteSpace(ViewModel.MonitorError))
        {
            return ViewModel.MonitorError!;
        }

        return ViewModel.MonitorState switch
        {
            "loading" => "Checking your sign-in status...",
            "running" => "Virtue is actively monitoring this device.",
            "starting" => "Virtue is bringing the background monitor online.",
            "signed_out" => "Monitoring resumes after you sign in again.",
            _ => ViewModel.StatusText,
        };
    }

    private void SetWindowIcon()
    {
        var iconCandidates = new[]
        {
            Path.Combine(AppContext.BaseDirectory, "Assets", "app-icon.ico"),
            Path.Combine(AppContext.BaseDirectory, "app-icon.ico"),
        };

        foreach (var path in iconCandidates)
        {
            if (File.Exists(path))
            {
                _appWindow.SetIcon(path);
                return;
            }
        }
    }

    private AppWindow ResolveAppWindow()
    {
        var windowHandle = WindowNative.GetWindowHandle(this);
        var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(windowHandle);
        return AppWindow.GetFromWindowId(windowId);
    }

    private void AppWindowOnClosing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (_allowClose)
        {
            return;
        }

        args.Cancel = true;
        HideToTray();
    }

    private static void OpenLogFolder(string logDirectory)
    {
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = logDirectory,
                UseShellExecute = true,
            });
        }
        catch (Exception ex)
        {
            // Nothing actionable for the user here — the path is printed in
            // the dialog either way, so a failed launch isn't worth an error
            // dialog on top of the status dialog.
            System.Diagnostics.Debug.WriteLine($"failed to open log folder: {ex}");
        }
    }

    private static string DisplayOrPlaceholder(string? value) =>
        string.IsNullOrWhiteSpace(value) ? "<none>" : value;

    private static string FormatUnixTimestamp(long? value)
    {
        if (!value.HasValue)
        {
            return "<none>";
        }

        return DateTimeOffset
            .FromUnixTimeMilliseconds(value.Value)
            .ToLocalTime()
            .ToString("yyyy-MM-dd HH:mm:ss zzz");
    }

    private static SolidColorBrush HexBrush(string value) => new(ColorFromHex(value));

    private static Windows.UI.Color ColorFromHex(string value)
    {
        var hex = value.TrimStart('#');
        if (hex.Length != 6)
        {
            throw new ArgumentException("Expected a 6-character hex value.", nameof(value));
        }

        return Windows.UI.Color.FromArgb(
            0xFF,
            Convert.ToByte(hex.Substring(0, 2), 16),
            Convert.ToByte(hex.Substring(2, 2), 16),
            Convert.ToByte(hex.Substring(4, 2), 16));
    }

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hWnd, ShowWindowCommands nCmdShow);

    private enum ShowWindowCommands
    {
        Hide = 0,
        Restore = 9,
    }
}
