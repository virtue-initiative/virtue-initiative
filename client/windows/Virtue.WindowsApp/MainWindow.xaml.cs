using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Virtue.WindowsApp.Core.ViewModels;
using Windows.Graphics;
using WinRT.Interop;

namespace Virtue.WindowsApp;

public sealed partial class MainWindow : Window
{
    private const string WebsiteDisplayUrl = "virtueinitiative.org";
    private const string WebsiteNavigateUrl = "https://virtueinitiative.org";

    private readonly AppWindow _appWindow;
    private readonly TextBlock _statusTextBlock;
    private readonly TextBlock _statusDetailTextBlock;
    private readonly TextBlock _buildLabelTextBlock;
    private readonly TextBlock _accountSummaryTextBlock;
    private readonly StackPanel _loginPanel;
    private readonly StackPanel _accountActionsPanel;
    private readonly StackPanel _signedInActionsPanel;
    private readonly TextBox _emailTextBox;
    private readonly PasswordBox _passwordBox;
    private readonly TextBox _deviceNameTextBox;
    private readonly Border _statusDot;
    private readonly TextBlock _errorTextBlock;
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

    public void HideToTray()
    {
        var windowHandle = WindowNative.GetWindowHandle(this);
        ShowWindow(windowHandle, ShowWindowCommands.Hide);
        IsVisibleToUser = false;
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

        var stopMonitoringButton = CreateActionButton("Stop Monitoring");
        stopMonitoringButton.Click += StopMonitoringButton_OnClick;

        _signedInActionsPanel.Orientation = Orientation.Horizontal;
        _signedInActionsPanel.Spacing = 10;
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

        var content = new StackPanel { Spacing = 8 };
        content.Children.Add(CreateSectionLabel("Status"));
        content.Children.Add(headingRow);
        content.Children.Add(_statusDetailTextBlock);
        content.Children.Add(_buildLabelTextBlock);
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
        _loginPanel.Children.Add(_passwordBox);
        _loginPanel.Children.Add(_deviceNameTextBox);

        _signInButton = CreatePrimaryButton("Sign In");
        _signInButton.Click += SignInButton_OnClick;
        _loginPanel.Children.Add(_signInButton);

        var signOutButton = CreateActionButton("Sign Out");
        signOutButton.Click += async (_, _) => await ViewModel.LogoutAsync();

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

    private string BuildStatusDetailsText()
    {
        var lines = new List<string>
        {
            $"logged_in: {ViewModel.LoggedIn.ToString().ToLowerInvariant()}",
            $"monitor_state: {ViewModel.MonitorState}",
            $"email: {DisplayOrPlaceholder(ViewModel.AccountEmail)}",
            $"device_id: {DisplayOrPlaceholder(ViewModel.DeviceId)}",
            $"pending_request_count: {ViewModel.PendingRequestCount}",
            $"last_screenshot_at: {FormatUnixTimestamp(ViewModel.LastScreenshotAtMs)}",
            $"build: {ViewModel.BuildLabel}",
        };

        if (!string.IsNullOrWhiteSpace(ViewModel.WindowsPackageVersion))
        {
            lines.Add($"windows_package: {ViewModel.WindowsPackageVersion}");
        }

        if (!string.IsNullOrWhiteSpace(ViewModel.MonitorError))
        {
            lines.Add($"last_error: {ViewModel.MonitorError}");
        }

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
        _statusTextBlock.Text = BuildPrimaryStatusText();
        var statusBrush = StatusBrush();
        _statusTextBlock.Foreground = statusBrush;
        _statusDot.Background = statusBrush;
        _statusDetailTextBlock.Text = BuildSecondaryStatusText();
        _buildLabelTextBlock.Text = ViewModel.BuildLabelText;

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
