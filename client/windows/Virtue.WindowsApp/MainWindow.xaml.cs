using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Virtue.WindowsApp.Core.Infrastructure;
using Virtue.WindowsApp.Core.Interop;
using Virtue.WindowsApp.Core.ViewModels;
using Windows.Graphics;
using Windows.System;
using WinRT.Interop;
using Path = Microsoft.UI.Xaml.Shapes.Path;

namespace Virtue.WindowsApp;

/// <summary>
/// The main window (CLIENT-027): a title bar with the logo, Home with one
/// focus area and a menu button, and the screens the menu opens.
/// </summary>
public sealed partial class MainWindow : Window
{
    private const string WebsiteNavigateUrl = "https://virtueinitiative.org";
    private const string SignUpNavigateUrl = "https://app.virtueinitiative.org/signup";
    // Partners are managed in the web app, not from this client.
    private const string PartnersNavigateUrl = "https://app.virtueinitiative.org/partners";

    // Short summary; the full list is on the linked help page. Keep both in step
    // with what the monitor actually does (core SPEC CORE-003, CORE-004).
    private const string MonitoringSummary =
        "Virtue takes a screenshot of every screen about every 5 minutes. Screenshots are blurred, have text blacked out, and can only be seen by you and your partners.";
    private const string MonitoringHelpUrl = "https://virtueinitiative.org/help/what-virtue-monitors/windows";

    // Brand tokens (shared-web tokens.css) plus the redesign's status colors.
    private static readonly SolidColorBrush BgBrush = HexBrush("#F4EFE3");
    private static readonly SolidColorBrush SurfaceBrush = HexBrush("#FBF7EA");
    private static readonly SolidColorBrush BgSubtleBrush = HexBrush("#EBE4CE");
    private static readonly SolidColorBrush BorderBrushToken = HexBrush("#D9D1BC");
    private static readonly SolidColorBrush BorderHoverBrush = HexBrush("#C9C0A8");
    private static readonly SolidColorBrush TextBrush = HexBrush("#1B1A16");
    private static readonly SolidColorBrush TextMutedBrush = HexBrush("#6A6655");
    private static readonly SolidColorBrush AccentBrush = HexBrush("#1E3A2E");
    private static readonly SolidColorBrush AccentHoverBrush = HexBrush("#2C4D3E");
    private static readonly SolidColorBrush AccentPressedBrush = HexBrush("#163026");
    private static readonly SolidColorBrush LogoBrush = HexBrush("#163026");
    private static readonly SolidColorBrush LinkBrush = HexBrush("#B34300");
    private static readonly SolidColorBrush GoodBrush = HexBrush("#2F7D4F");
    private static readonly SolidColorBrush BadBrush = HexBrush("#C0392B");
    private static readonly SolidColorBrush BadHoverBrush = HexBrush("#A93226");
    private static readonly SolidColorBrush WarnBrush = HexBrush("#A67F3D");
    private static readonly SolidColorBrush NeutralBrush = HexBrush("#A8A290");
    private static readonly SolidColorBrush OnAccentBrush = new(Colors.White);
    private static readonly SolidColorBrush ToastBrush = HexBrush("#1B1A16");

    private enum Screen
    {
        Home,
        Details,
        Monitors,
        Report,
    }

    private enum StatusKind
    {
        Good,
        Warn,
        Bad,
        Neutral,
    }

    private readonly AppWindow _appWindow;
    private Screen _screen = Screen.Home;
    private bool _allowClose;
    private bool _showAdvanced;
    private bool _detailsRenderQueued;
    private int _detailsColumns;

    // Title bar.
    private readonly Grid _dragRegion = new();
    private readonly HyperlinkButton _brandLink = new();
    private readonly StackPanel _backTitle = new() { Orientation = Orientation.Horizontal, Spacing = 4 };
    private readonly TextBlock _screenTitle = new();

    // Content shared by every screen.
    private readonly Grid _screenHost = new();
    private readonly InfoBar _updateInfoBar = new();
    private readonly Button _restartNowButton = new() { Content = "Restart now to update" };
    private readonly Border _toast = new();
    private readonly TextBlock _toastText = new();

    // Home.
    private readonly Grid _homeView = new();
    private readonly Button _menuButton = new();
    private readonly StackPanel _signInPanel = new();
    private readonly StackPanel _statusPanel = new();
    private readonly Border _ring = new();
    private readonly TextBlock _headline = new();
    private readonly TextBlock _sentence = new();
    private readonly Button _partnersButton = CreatePrimaryButton(string.Empty);
    private readonly Button _reportFromHomeButton = CreatePrimaryButton("Report an Issue");
    private readonly Button _stopButton = CreateSecondaryButton("Stop Monitoring");
    private readonly InfoBar _homeError = CreateErrorBar();
    private readonly InfoBar _signInError = CreateErrorBar();
    private readonly TextBox _emailTextBox = new();
    private readonly PasswordBox _passwordBox = new();
    private readonly TextBox _deviceNameTextBox = new();
    private readonly Button _signInButton = CreatePrimaryButton("Sign In");
    private string? _ringKey;

    // Sub-screens.
    private readonly ScrollViewer _detailsView = new();
    private readonly StackPanel _detailsContent = new() { Padding = new Thickness(28, 24, 28, 36) };
    private readonly ScrollViewer _monitorsView = new();
    private readonly ScrollViewer _reportView = new();
    private readonly TextBox _reportMessageBox = new();
    private readonly TextBox _reportEmailBox = new();
    private readonly CheckBox _reportLogsCheckBox = new();
    private readonly InfoBar _reportError = CreateErrorBar();
    private readonly Button _sendReportButton = CreatePrimaryButton("Send Report");

    public MainWindow(SessionViewModel viewModel)
    {
        ViewModel = viewModel;
        Title = "Virtue";

        _appWindow = ResolveAppWindow();
        Content = BuildContent();
        ConfigureTitleBar();
        ViewModel.PropertyChanged += ViewModelOnPropertyChanged;
        _appWindow.Closing += AppWindowOnClosing;
        SetInitialBounds();
        SetWindowIcon();
        Navigate(Screen.Home);
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
    /// Raised by the window menu's "Check for Updates" item. Its only effect is to
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

    /// <summary>Opens Report an Issue in the window (the tray's Report an Issue, CLIENT-027).</summary>
    public void ShowReportIssue()
    {
        Navigate(Screen.Report);
    }

    // ---- Layout ------------------------------------------------------------

    private UIElement BuildContent()
    {
        var root = new Grid { Background = BgBrush, RequestedTheme = ElementTheme.Light };
        // WinUI would focus the first tab stop, the logo link, and draw its
        // keyboard focus ring as the window opens. Start on the content instead.
        root.Loaded += (_, _) =>
        {
            if (_signInPanel.Visibility == Visibility.Visible)
            {
                _emailTextBox.Focus(FocusState.Programmatic);
            }
            else
            {
                _menuButton.Focus(FocusState.Pointer);
            }
        };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var titleBar = BuildTitleBar();
        Grid.SetRow(titleBar, 0);
        root.Children.Add(titleBar);

        var body = new Grid();
        body.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        body.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        Grid.SetRow(body, 1);
        root.Children.Add(body);

        _restartNowButton.Click += (_, _) => CloseNowAndUpdateRequested?.Invoke(this, EventArgs.Empty);
        _updateInfoBar.Title = "Update ready";
        _updateInfoBar.Severity = InfoBarSeverity.Informational;
        _updateInfoBar.IsClosable = false;
        _updateInfoBar.ActionButton = _restartNowButton;
        _updateInfoBar.Margin = new Thickness(16, 12, 16, 0);
        Grid.SetRow(_updateInfoBar, 0);
        body.Children.Add(_updateInfoBar);

        _screenHost.Children.Add(BuildHome());
        _screenHost.Children.Add(BuildDetails());
        _screenHost.Children.Add(BuildMonitors());
        _screenHost.Children.Add(BuildReport());
        Grid.SetRow(_screenHost, 1);
        body.Children.Add(_screenHost);

        _toastText.Foreground = OnAccentBrush;
        _toastText.FontSize = 13;
        _toast.Child = _toastText;
        _toast.Background = ToastBrush;
        _toast.CornerRadius = new CornerRadius(6);
        _toast.Padding = new Thickness(16, 8, 16, 8);
        _toast.HorizontalAlignment = HorizontalAlignment.Center;
        _toast.VerticalAlignment = VerticalAlignment.Bottom;
        _toast.Margin = new Thickness(0, 0, 0, 20);
        _toast.IsHitTestVisible = false;
        AutomationProperties.SetLiveSetting(_toastText, Microsoft.UI.Xaml.Automation.Peers.AutomationLiveSetting.Polite);
        Grid.SetRow(_toast, 1);
        body.Children.Add(_toast);

        return root;
    }

    /// <summary>
    /// The custom title bar: the logo link on Home, or a back button and the
    /// screen title elsewhere. Only <see cref="_dragRegion"/> drags the window,
    /// so the link and back button stay clickable.
    /// </summary>
    private FrameworkElement BuildTitleBar()
    {
        var bar = new Grid
        {
            Height = 48,
            Background = SurfaceBrush,
            BorderBrush = BorderBrushToken,
            BorderThickness = new Thickness(0, 0, 0, 1),
        };
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        var brand = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        brand.Children.Add(CreateLogo(20));
        brand.Children.Add(new TextBlock
        {
            Text = "Virtue Initiative",
            FontSize = 13,
            FontWeight = FontWeights.SemiBold,
            Foreground = TextBrush,
            VerticalAlignment = VerticalAlignment.Center,
        });
        _brandLink.Content = brand;
        _brandLink.NavigateUri = new Uri(WebsiteNavigateUrl);
        _brandLink.Padding = new Thickness(6, 4, 8, 4);
        _brandLink.Margin = new Thickness(8, 0, 0, 0);
        _brandLink.VerticalAlignment = VerticalAlignment.Center;
        _brandLink.Resources["HyperlinkButtonBackgroundPointerOver"] = BgSubtleBrush;
        _brandLink.Resources["HyperlinkButtonBackgroundPressed"] = BorderBrushToken;
        AutomationProperties.SetName(_brandLink, "Virtue Initiative website");
        ToolTipService.SetToolTip(_brandLink, "virtueinitiative.org");

        var backButton = CreateIconButton("", "Back", 36, 32);
        backButton.Click += (_, _) => Navigate(Screen.Home);
        backButton.KeyboardAccelerators.Add(new KeyboardAccelerator { Key = VirtualKey.Left, Modifiers = VirtualKeyModifiers.Menu });
        backButton.KeyboardAccelerators.Add(new KeyboardAccelerator { Key = VirtualKey.GoBack });
        _screenTitle.FontSize = 13;
        _screenTitle.FontWeight = FontWeights.SemiBold;
        _screenTitle.Foreground = TextBrush;
        _screenTitle.VerticalAlignment = VerticalAlignment.Center;
        _backTitle.Margin = new Thickness(6, 0, 0, 0);
        _backTitle.VerticalAlignment = VerticalAlignment.Center;
        _backTitle.Children.Add(backButton);
        _backTitle.Children.Add(_screenTitle);

        var left = new Grid();
        left.Children.Add(_brandLink);
        left.Children.Add(_backTitle);
        Grid.SetColumn(left, 0);
        bar.Children.Add(left);

        // Transparent so it receives the pointer, which is what makes it draggable.
        _dragRegion.Background = new SolidColorBrush(Colors.Transparent);
        Grid.SetColumn(_dragRegion, 1);
        bar.Children.Add(_dragRegion);
        return bar;
    }

    private void ConfigureTitleBar()
    {
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(_dragRegion);
        var titleBar = _appWindow.TitleBar;
        titleBar.PreferredHeightOption = TitleBarHeightOption.Tall;
        titleBar.ButtonBackgroundColor = Colors.Transparent;
        titleBar.ButtonInactiveBackgroundColor = Colors.Transparent;
        titleBar.ButtonForegroundColor = TextBrush.Color;
        titleBar.ButtonInactiveForegroundColor = TextMutedBrush.Color;
        titleBar.ButtonHoverBackgroundColor = BgSubtleBrush.Color;
        titleBar.ButtonHoverForegroundColor = TextBrush.Color;
        titleBar.ButtonPressedBackgroundColor = BorderBrushToken.Color;
        titleBar.ButtonPressedForegroundColor = TextBrush.Color;
    }

    private UIElement BuildHome()
    {
        _homeView.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        _homeView.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var menuIcon = new FontIcon { Glyph = "", FontSize = 16, Foreground = TextBrush };
        _menuButton.Content = menuIcon;
        StyleIconButton(_menuButton, 40, 36);
        _menuButton.HorizontalAlignment = HorizontalAlignment.Right;
        _menuButton.Margin = new Thickness(0, 10, 14, 0);
        AutomationProperties.SetName(_menuButton, "Menu");
        ToolTipService.SetToolTip(_menuButton, "Menu");
        var menu = new MenuFlyout { Placement = FlyoutPlacementMode.BottomEdgeAlignedRight };
        menu.MenuFlyoutPresenterStyle = CreateMenuPresenterStyle();
        menu.Opening += (_, _) => FillMenu(menu);
        _menuButton.Flyout = menu;
        Grid.SetRow(_menuButton, 0);
        _homeView.Children.Add(_menuButton);

        var focus = new Grid { Padding = new Thickness(24, 4, 24, 40) };
        focus.Children.Add(BuildSignInPanel());
        focus.Children.Add(BuildStatusPanel());

        var scroll = new ScrollViewer { Content = focus, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
        // Lets the status panel center vertically while still scrolling when
        // the window is too short for it.
        scroll.SizeChanged += (_, e) => focus.MinHeight = e.NewSize.Height;
        Grid.SetRow(scroll, 1);
        _homeView.Children.Add(scroll);
        return _homeView;
    }

    private UIElement BuildSignInPanel()
    {
        _signInPanel.Spacing = 12;
        _signInPanel.MaxWidth = 400;
        _signInPanel.VerticalAlignment = VerticalAlignment.Top;

        _signInPanel.Children.Add(new TextBlock
        {
            Text = "Sign in",
            FontSize = 28,
            FontWeight = FontWeights.SemiBold,
            Foreground = TextBrush,
        });
        _signInPanel.Children.Add(new TextBlock
        {
            Text = "Sign in to start monitoring on this device.",
            FontSize = 14,
            Foreground = TextMutedBrush,
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, -4, 0, 4),
        });

        _emailTextBox.PlaceholderText = "Email";
        _emailTextBox.InputScope = new InputScope { Names = { new InputScopeName(InputScopeNameValue.EmailSmtpAddress) } };
        _emailTextBox.TextChanged += (_, _) =>
        {
            ViewModel.EmailInput = _emailTextBox.Text;
            ClearStaleError();
        };
        AutomationProperties.SetName(_emailTextBox, "Email");
        StyleInput(_emailTextBox);
        _signInPanel.Children.Add(_emailTextBox);

        _signInPanel.Children.Add(BuildPasswordField());

        _deviceNameTextBox.PlaceholderText = $"Device name (default \"{Environment.MachineName}\")";
        _deviceNameTextBox.Text = ViewModel.DeviceNameInput;
        _deviceNameTextBox.TextChanged += (_, _) => ViewModel.DeviceNameInput = _deviceNameTextBox.Text;
        AutomationProperties.SetName(_deviceNameTextBox, "Device name");
        StyleInput(_deviceNameTextBox);
        _signInPanel.Children.Add(_deviceNameTextBox);

        foreach (var input in new Control[] { _emailTextBox, _passwordBox, _deviceNameTextBox })
        {
            input.KeyDown += async (_, e) =>
            {
                if (e.Key == VirtualKey.Enter)
                {
                    e.Handled = true;
                    await ViewModel.LoginAsync();
                }
            };
        }

        _signInButton.HorizontalAlignment = HorizontalAlignment.Stretch;
        _signInButton.Margin = new Thickness(0, 4, 0, 0);
        _signInButton.Click += async (_, _) => await ViewModel.LoginAsync();
        _signInPanel.Children.Add(_signInButton);

        var signUpLink = CreateLink("Don't have an account? Sign up", SignUpNavigateUrl);
        signUpLink.HorizontalAlignment = HorizontalAlignment.Center;
        _signInPanel.Children.Add(signUpLink);
        _signInPanel.Children.Add(_signInError);
        return _signInPanel;
    }

    /// <summary>
    /// The password box with an eye button drawn inside it. `Peek` would draw
    /// WinUI's own reveal button, but only while the box has focus, so the
    /// reveal is driven from this button instead.
    /// </summary>
    private UIElement BuildPasswordField()
    {
        var field = new Grid();
        _passwordBox.PlaceholderText = "Password";
        _passwordBox.PasswordRevealMode = PasswordRevealMode.Hidden;
        _passwordBox.PasswordChanged += (_, _) =>
        {
            ViewModel.PasswordInput = _passwordBox.Password;
            ClearStaleError();
        };
        AutomationProperties.SetName(_passwordBox, "Password");
        StyleInput(_passwordBox);
        _passwordBox.Padding = new Thickness(10, 6, 44, 6);
        field.Children.Add(_passwordBox);

        var revealIcon = new FontIcon { Glyph = "", FontSize = 14, Foreground = TextMutedBrush };
        var revealButton = new Button { Content = revealIcon };
        StyleIconButton(revealButton, 34, 28);
        revealButton.HorizontalAlignment = HorizontalAlignment.Right;
        revealButton.VerticalAlignment = VerticalAlignment.Center;
        revealButton.Margin = new Thickness(0, 0, 4, 0);
        revealButton.IsTabStop = true;
        AutomationProperties.SetName(revealButton, "Show password");
        ToolTipService.SetToolTip(revealButton, "Show password");
        revealButton.Click += (_, _) =>
        {
            var reveal = _passwordBox.PasswordRevealMode != PasswordRevealMode.Visible;
            _passwordBox.PasswordRevealMode = reveal ? PasswordRevealMode.Visible : PasswordRevealMode.Hidden;
            revealIcon.Glyph = reveal ? "" : "";
            var label = reveal ? "Hide password" : "Show password";
            AutomationProperties.SetName(revealButton, label);
            ToolTipService.SetToolTip(revealButton, label);
        };
        field.Children.Add(revealButton);
        return field;
    }

    private UIElement BuildStatusPanel()
    {
        _statusPanel.Spacing = 14;
        _statusPanel.MaxWidth = 400;
        _statusPanel.VerticalAlignment = VerticalAlignment.Center;
        _statusPanel.HorizontalAlignment = HorizontalAlignment.Center;

        _ring.Width = 112;
        _ring.Height = 112;
        _ring.CornerRadius = new CornerRadius(56);
        _ring.BorderThickness = new Thickness(6);
        _ring.Background = SurfaceBrush;
        _ring.HorizontalAlignment = HorizontalAlignment.Center;
        _ring.Margin = new Thickness(0, 0, 0, 4);
        _statusPanel.Children.Add(_ring);

        _headline.FontSize = 28;
        _headline.FontWeight = FontWeights.SemiBold;
        _headline.Foreground = TextBrush;
        _headline.TextAlignment = TextAlignment.Center;
        _headline.TextWrapping = TextWrapping.Wrap;
        _headline.HorizontalAlignment = HorizontalAlignment.Center;
        AutomationProperties.SetHeadingLevel(_headline, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level1);
        _statusPanel.Children.Add(_headline);

        _sentence.FontSize = 14;
        _sentence.Foreground = TextMutedBrush;
        _sentence.TextAlignment = TextAlignment.Center;
        _sentence.TextWrapping = TextWrapping.Wrap;
        _sentence.MaxWidth = 320;
        _sentence.HorizontalAlignment = HorizontalAlignment.Center;
        _statusPanel.Children.Add(_sentence);

        _partnersButton.Click += (_, _) => OpenInBrowser(PartnersNavigateUrl);
        _reportFromHomeButton.Click += (_, _) => Navigate(Screen.Report);
        _stopButton.Click += async (_, _) =>
        {
            if (Application.Current is App app)
            {
                await app.RequestResidentShutdownAsync();
            }
        };

        var actions = new StackPanel { Spacing = 10, Width = 280, Margin = new Thickness(0, 10, 0, 0) };
        foreach (var button in new[] { _partnersButton, _reportFromHomeButton, _stopButton })
        {
            button.HorizontalAlignment = HorizontalAlignment.Stretch;
            actions.Children.Add(button);
        }
        _statusPanel.Children.Add(actions);

        _homeError.Margin = new Thickness(0, 6, 0, 0);
        _homeError.Width = 320;
        _statusPanel.Children.Add(_homeError);
        return _statusPanel;
    }

    private void FillMenu(MenuFlyout menu)
    {
        menu.Items.Clear();
        if (ViewModel.LoggedIn)
        {
            var test = CreateMenuItem("Test Screenshot", "");
            test.IsEnabled = !ViewModel.IsBusy;
            test.Click += async (_, _) => await ForceCaptureAsync();
            menu.Items.Add(test);
            menu.Items.Add(new MenuFlyoutSeparator());
        }

        var details = CreateMenuItem("Status Details", "");
        details.Click += (_, _) => Navigate(Screen.Details);
        menu.Items.Add(details);

        var monitors = CreateMenuItem("What Virtue Monitors", "");
        monitors.Click += (_, _) => Navigate(Screen.Monitors);
        menu.Items.Add(monitors);

        var report = CreateMenuItem("Report an Issue", "");
        report.Click += (_, _) => Navigate(Screen.Report);
        menu.Items.Add(report);

        var updates = CreateMenuItem("Check for Updates", "");
        updates.Click += (_, _) => CheckForUpdatesRequested?.Invoke(this, EventArgs.Empty);
        menu.Items.Add(updates);

        if (ViewModel.LoggedIn)
        {
            menu.Items.Add(new MenuFlyoutSeparator());
            var signOut = CreateMenuItem("Sign Out", "");
            signOut.Foreground = BadBrush;
            signOut.Resources["MenuFlyoutItemForeground"] = BadBrush;
            signOut.Resources["MenuFlyoutItemForegroundPointerOver"] = BadBrush;
            signOut.Resources["MenuFlyoutItemForegroundPressed"] = BadBrush;
            if (signOut.Icon is FontIcon icon)
            {
                icon.Foreground = BadBrush;
            }
            signOut.IsEnabled = !ViewModel.IsBusy;
            signOut.Click += async (_, _) =>
            {
                if (await ShowLogoutConfirmationAsync())
                {
                    await ViewModel.LogoutAsync();
                }
            };
            menu.Items.Add(signOut);
        }

        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(CreateMenuNote(ViewModel.BuildLabelText));
        if (!string.IsNullOrWhiteSpace(ViewModel.UpdateCheckStatusText))
        {
            menu.Items.Add(CreateMenuNote(ViewModel.UpdateCheckStatusText!));
        }
    }

    private UIElement BuildMonitors()
    {
        var card = new StackPanel { Spacing = 10 };
        card.Children.Add(new TextBlock
        {
            Text = MonitoringSummary,
            TextWrapping = TextWrapping.Wrap,
            FontSize = 14,
            Foreground = TextBrush,
            LineHeight = 21,
        });
        card.Children.Add(CreateLink("Learn more", MonitoringHelpUrl));

        _monitorsView.Content = CreateColumn(CreateCard(card));
        _monitorsView.VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
        return _monitorsView;
    }

    private UIElement BuildReport()
    {
        var column = new StackPanel { Spacing = 12 };

        _reportMessageBox.PlaceholderText = "Describe the issue";
        _reportMessageBox.AcceptsReturn = true;
        _reportMessageBox.TextWrapping = TextWrapping.Wrap;
        _reportMessageBox.Height = 140;
        AutomationProperties.SetName(_reportMessageBox, "Describe the issue");
        StyleInput(_reportMessageBox);
        column.Children.Add(_reportMessageBox);

        _reportEmailBox.PlaceholderText = "Contact email (optional)";
        _reportEmailBox.InputScope = new InputScope { Names = { new InputScopeName(InputScopeNameValue.EmailSmtpAddress) } };
        AutomationProperties.SetName(_reportEmailBox, "Contact email (optional)");
        StyleInput(_reportEmailBox);
        column.Children.Add(_reportEmailBox);

        _reportLogsCheckBox.Content = "Include the last day of diagnostic logs";
        _reportLogsCheckBox.IsChecked = true;
        StyleCheckBox(_reportLogsCheckBox);
        var logs = new StackPanel { Spacing = 0 };
        logs.Children.Add(_reportLogsCheckBox);
        logs.Children.Add(new TextBlock
        {
            Text = "Includes timestamps, monitoring status, and error messages from the last day. " +
                   "No screenshots or window titles are included. Known tokens are redacted automatically.",
            TextWrapping = TextWrapping.Wrap,
            FontSize = 12,
            Foreground = TextMutedBrush,
            Margin = new Thickness(28, 0, 0, 0),
        });
        column.Children.Add(logs);

        column.Children.Add(_reportError);

        _sendReportButton.HorizontalAlignment = HorizontalAlignment.Stretch;
        _sendReportButton.Click += async (_, _) => await SendReportAsync();
        column.Children.Add(_sendReportButton);

        _reportView.Content = CreateColumn(column);
        _reportView.VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
        return _reportView;
    }

    private UIElement BuildDetails()
    {
        _detailsView.Content = _detailsContent;
        _detailsView.VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
        _detailsContent.MaxWidth = 840;
        _detailsView.SizeChanged += (_, _) =>
        {
            if (_screen == Screen.Details && ColumnsFor(_detailsView.ActualWidth) != _detailsColumns)
            {
                RenderDetails();
            }
        };
        return _detailsView;
    }

    /// <summary>An error about what was typed goes away once the user edits it.</summary>
    private void ClearStaleError()
    {
        if (!ViewModel.IsBusy && ViewModel.ErrorText is not null)
        {
            ViewModel.ClearError();
        }
    }

    // ---- Navigation --------------------------------------------------------

    private void Navigate(Screen screen)
    {
        _screen = screen;
        ViewModel.ClearError();
        _homeView.Visibility = VisibleIf(screen == Screen.Home);
        _detailsView.Visibility = VisibleIf(screen == Screen.Details);
        _monitorsView.Visibility = VisibleIf(screen == Screen.Monitors);
        _reportView.Visibility = VisibleIf(screen == Screen.Report);

        _brandLink.Visibility = VisibleIf(screen == Screen.Home);
        _backTitle.Visibility = VisibleIf(screen != Screen.Home);
        _screenTitle.Text = screen switch
        {
            Screen.Details => "Status Details",
            Screen.Monitors => "What Virtue Monitors",
            Screen.Report => "Report an Issue",
            _ => string.Empty,
        };

        switch (screen)
        {
            case Screen.Details:
                _showAdvanced = false;
                RenderDetails();
                _detailsView.ChangeView(null, 0, null, true);
                _ = ViewModel.RefreshAsync();
                break;
            case Screen.Report:
                _reportMessageBox.Text = string.Empty;
                _reportEmailBox.Text = ViewModel.LoggedIn ? ViewModel.AccountEmail : string.Empty;
                _reportLogsCheckBox.IsChecked = true;
                _reportError.IsOpen = false;
                _reportMessageBox.Focus(FocusState.Programmatic);
                break;
        }

        SyncFromViewModel();
    }

    // ---- State -------------------------------------------------------------

    private void ViewModelOnPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        SyncFromViewModel();
    }

    private void SyncFromViewModel()
    {
        _updateInfoBar.IsOpen = ViewModel.UpdateReady;
        _updateInfoBar.Message = ViewModel.UpdateInstalling
            ? "Installing the update. Virtue will close and restart itself within a minute."
            : ViewModel.UpdateCountdownText is { } countdown
                ? $"Virtue will restart to update in {countdown}."
                : "An update is ready and will install soon.";
        _restartNowButton.IsEnabled = !ViewModel.UpdateInstalling;

        var signedOut = ViewModel.HasLoadedStatus && !ViewModel.LoggedIn;
        _signInPanel.Visibility = VisibleIf(signedOut);
        _statusPanel.Visibility = VisibleIf(!signedOut);

        var kind = HomeKind();
        UpdateRing(kind);
        _headline.Text = ViewModel.Headline;
        _sentence.Text = ViewModel.StatusSentence;

        var partnerCount = ViewModel.MonitorStatus?.PartnerCount;
        _partnersButton.Content = partnerCount switch
        {
            null => "Manage partners",
            0 => "Add partners",
            _ => "Add more partners",
        };
        var needsAttention = ViewModel.MonitorState is "error" or "stopped";
        var signedIn = ViewModel.HasLoadedStatus && ViewModel.LoggedIn;
        _partnersButton.Visibility = VisibleIf(signedIn && !needsAttention);
        _reportFromHomeButton.Visibility = VisibleIf(signedIn && needsAttention);
        _stopButton.Visibility = VisibleIf(signedIn);

        var busy = ViewModel.IsBusy;
        _emailTextBox.IsEnabled = !busy;
        _passwordBox.IsEnabled = !busy;
        _deviceNameTextBox.IsEnabled = !busy;
        _signInButton.IsEnabled = !busy;
        _signInButton.Content = busy && ViewModel.TransitionMessage == "Signing in..." ? "Signing in..." : "Sign In";
        _sendReportButton.IsEnabled = !busy;
        _sendReportButton.Content = busy && ViewModel.TransitionMessage == "Sending report..." ? "Sending..." : "Send Report";

        var error = ViewModel.ErrorText;
        SetError(_signInError, _screen == Screen.Home && signedOut ? error : null);
        SetError(_homeError, _screen == Screen.Home && !signedOut ? error : null);

        // The sign-in and report buttons already say what they are doing.
        var toast = ViewModel.TransitionMessage;
        var showToast = toast is not null && toast != "Signing in..." && toast != "Sending report...";
        _toastText.Text = toast ?? string.Empty;
        _toast.Visibility = VisibleIf(showToast);

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

        if (_screen == Screen.Details)
        {
            QueueDetailsRender();
        }
    }

    private StatusKind HomeKind() =>
        ViewModel.MonitorState switch
        {
            "loading" or "signed_out" => StatusKind.Neutral,
            "error" or "stopped" => StatusKind.Bad,
            _ => ViewModel.LastScreenshotAtMs is null ? StatusKind.Warn : StatusKind.Good,
        };

    private void UpdateRing(StatusKind kind)
    {
        var key = kind.ToString();
        if (_ringKey == key)
        {
            return;
        }

        _ringKey = key;
        var brush = KindBrush(kind);
        _ring.BorderBrush = brush;
        _ring.Child = kind switch
        {
            StatusKind.Good => CreateGlyph("M5,12.5 L9.5,17 L19,7.5", brush, 56, 2.4),
            StatusKind.Bad => CreateExclamation(brush),
            _ => CreateThreeDots(brush),
        };
    }

    // ---- Status Details ----------------------------------------------------

    private static int ColumnsFor(double width) => width >= 680 ? 2 : 1;

    /// <summary>Several properties change per refresh; render once after them.</summary>
    private void QueueDetailsRender()
    {
        if (_detailsRenderQueued)
        {
            return;
        }

        _detailsRenderQueued = true;
        DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
        {
            _detailsRenderQueued = false;
            if (_screen == Screen.Details)
            {
                RenderDetails();
            }
        });
    }

    private void RenderDetails()
    {
        _detailsColumns = ColumnsFor(_detailsView.ActualWidth);
        var children = _detailsContent.Children;
        children.Clear();
        children.Add(BuildDetailsSummary());

        var sections = new[] { BuildScreenshotsSection(), BuildAccountSection(), BuildProblemsSection() };
        var grid = new Grid { ColumnSpacing = 24, RowSpacing = 28, Margin = new Thickness(0, 0, 0, 28) };
        for (var c = 0; c < _detailsColumns; c++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        }

        for (var i = 0; i < sections.Length; i++)
        {
            if (i % _detailsColumns == 0)
            {
                grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            }

            Grid.SetRow(sections[i], i / _detailsColumns);
            Grid.SetColumn(sections[i], i % _detailsColumns);
            grid.Children.Add(sections[i]);
        }
        children.Add(grid);
        children.Add(BuildAdvanced());
    }

    private UIElement BuildDetailsSummary()
    {
        var panel = new Grid
        {
            ColumnSpacing = 12,
            RowSpacing = 4,
            Padding = new Thickness(2, 8, 2, 24),
            Margin = new Thickness(0, 0, 0, 28),
            BorderBrush = BorderBrushToken,
            BorderThickness = new Thickness(0, 0, 0, 1),
        };
        panel.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        panel.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        panel.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        panel.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var dot = CreateDot(HomeKind(), 14);
        panel.Children.Add(dot);

        var headline = new TextBlock
        {
            Text = ViewModel.Headline,
            FontSize = 26,
            FontWeight = FontWeights.SemiBold,
            Foreground = TextBrush,
            TextWrapping = TextWrapping.Wrap,
        };
        AutomationProperties.SetHeadingLevel(headline, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level1);
        Grid.SetColumn(headline, 1);
        panel.Children.Add(headline);

        var sentence = new TextBlock
        {
            Text = DetailsSentence(),
            FontSize = 14,
            Foreground = TextMutedBrush,
            TextWrapping = TextWrapping.Wrap,
        };
        Grid.SetRow(sentence, 1);
        Grid.SetColumn(sentence, 1);
        panel.Children.Add(sentence);
        return panel;
    }

    private string DetailsSentence() =>
        ViewModel.MonitorState switch
        {
            "signed_out" => "Sign in to start monitoring.",
            "error" or "stopped" or "loading" => ViewModel.StatusSentence,
            _ => ViewModel.LastScreenshotAtMs is null
                ? "Waiting for the first screenshot."
                : "Everything is working. Screenshots are being taken and uploaded.",
        };

    private FrameworkElement BuildScreenshotsSection()
    {
        var status = ViewModel.MonitorStatus;
        var signedIn = ViewModel.LoggedIn;
        var rows = new List<UIElement>();
        if (ViewModel.LastScreenshotAtMs is long last)
        {
            rows.Add(CreateRow(StatusKind.Good, "Last screenshot", SessionViewModel.FormatShortTime(last)));
        }
        else if (signedIn)
        {
            rows.Add(CreateRow(StatusKind.Warn, "Last screenshot", "None yet", "The first one is taken within a few minutes."));
        }
        else
        {
            rows.Add(CreateRow(StatusKind.Neutral, "Last screenshot", "None"));
        }

        if (status is null)
        {
            rows.Add(CreateRow(StatusKind.Neutral, "Waiting to upload", "Unknown"));
        }
        else
        {
            var waiting = status.PendingHashCount + status.PendingBatchCount;
            rows.Add(CreateRow(signedIn ? StatusKind.Good : StatusKind.Neutral, "Waiting to upload", waiting.ToString()));
        }

        return CreateSection("Screenshots", rows);
    }

    private FrameworkElement BuildAccountSection()
    {
        var rows = new List<UIElement>();
        if (ViewModel.LoggedIn)
        {
            rows.Add(CreateRow(StatusKind.Good, "Signed in", string.IsNullOrWhiteSpace(ViewModel.AccountEmail) ? "Yes" : ViewModel.AccountEmail));
            var count = ViewModel.MonitorStatus?.PartnerCount;
            rows.Add(count switch
            {
                null => CreateRow(StatusKind.Neutral, "Partners", "Unknown"),
                0 => CreateRow(StatusKind.Warn, "Partners", "0", "Add a partner on the Virtue website so someone receives your reports."),
                _ => CreateRow(StatusKind.Good, "Partners", count.Value.ToString()),
            });
        }
        else
        {
            rows.Add(CreateRow(StatusKind.Neutral, "Signed in", ViewModel.HasLoadedStatus ? "No" : "Loading..."));
        }

        return CreateSection("Account", rows);
    }

    private FrameworkElement BuildProblemsSection()
    {
        var status = ViewModel.MonitorStatus;
        UIElement row;
        if (ViewModel.MonitorState == "error" && !string.IsNullOrWhiteSpace(ViewModel.MonitorError))
        {
            row = CreateRow(StatusKind.Bad, "Last problem", "Monitoring could not start", ViewModel.MonitorError);
        }
        else if (status?.RecentErrors is { Count: > 0 } errors)
        {
            // A past error needs no action, so it reads as a warning for a
            // day and as neutral after that.
            var newest = errors.OrderByDescending(e => e.AtMs).First();
            var recent = DateTimeOffset.UtcNow - DateTimeOffset.FromUnixTimeMilliseconds(newest.AtMs) < TimeSpan.FromDays(1);
            row = CreateRow(recent ? StatusKind.Warn : StatusKind.Neutral, "Last problem", SessionViewModel.FormatShortTime(newest.AtMs), newest.Message);
        }
        else if (status is null)
        {
            row = CreateRow(StatusKind.Neutral, "Recent problems", "Unknown");
        }
        else
        {
            row = CreateRow(StatusKind.Good, "Recent problems", "None");
        }

        return CreateSection("Problems", new List<UIElement> { row });
    }

    private UIElement BuildAdvanced()
    {
        var panel = new StackPanel { Spacing = 8 };

        var chevron = new FontIcon { Glyph = _showAdvanced ? "" : "", FontSize = 11, Foreground = TextMutedBrush };
        var toggleContent = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        toggleContent.Children.Add(new TextBlock { Text = "Advanced", FontSize = 14, Foreground = TextMutedBrush });
        toggleContent.Children.Add(chevron);
        var toggle = new Button { Content = toggleContent, Padding = new Thickness(4, 6, 6, 6) };
        StyleQuietButton(toggle);
        AutomationProperties.SetName(toggle, _showAdvanced ? "Hide Advanced" : "Show Advanced");
        toggle.Click += (_, _) =>
        {
            _showAdvanced = !_showAdvanced;
            RenderDetails();
        };
        panel.Children.Add(toggle);

        if (_showAdvanced)
        {
            var rows = AdvancedRows().Select(r => (UIElement)CreateRow(null, r.Label, r.Value)).ToList();
            var logDirectory = ViewModel.MonitorStatus?.LogDirectory;
            if (!string.IsNullOrWhiteSpace(logDirectory))
            {
                var open = CreateSecondaryButton("Open log folder");
                open.Click += (_, _) => OpenLogFolder(logDirectory!);
                rows.Add(new Border { Padding = new Thickness(14, 12, 14, 12), Child = open });
            }
            panel.Children.Add(CreateGroup(rows));
        }

        return panel;
    }

    private List<(string Label, string Value)> AdvancedRows()
    {
        var status = ViewModel.MonitorStatus;
        var rows = new List<(string Label, string Value)>
        {
            ("Device name", DisplayOrPlaceholder(status?.DeviceName)),
            ("Device ID", DisplayOrPlaceholder(ViewModel.DeviceId)),
            ("API URL", DisplayOrPlaceholder(status?.ApiBaseUrl)),
            ("Hash base URL", string.IsNullOrWhiteSpace(status?.HashBaseUrl) ? "<default>" : status!.HashBaseUrl!),
            ("Capture interval", status?.CaptureIntervalSeconds is long capture ? $"{capture}s" : "<unknown>"),
            ("Batch window", status?.BatchWindowSeconds is long batch ? $"{batch}s" : "<unknown>"),
            ("Last upload", FormatTimestamp(status?.LastBatchAtMs)),
            ("Waiting for hash", (status?.PendingHashCount ?? 0).ToString()),
            ("Waiting in batch", (status?.PendingBatchCount ?? 0).ToString()),
            ("Pending requests", ViewModel.PendingRequestCount.ToString()),
            ("Monitor state", ViewModel.MonitorStateDisplay),
            ("Last loop", FormatTimestamp(status?.LastLoopAtMs)),
            ("Last attempt", FormatTimestamp(status?.LastScreenshotAttemptAtMs)),
            ("Last screenshot", FormatTimestamp(ViewModel.LastScreenshotAtMs)),
            ("Last skip reason", SkipReasonLabel(status?.LastSkipReason)),
        };

        var errors = status?.RecentErrors;
        if (errors is null || errors.Count == 0)
        {
            rows.Add(("Recent errors", "<none>"));
        }
        else
        {
            foreach (var error in errors.OrderByDescending(e => e.AtMs))
            {
                rows.Add(($"{FormatTimestamp(error.AtMs)} · {error.Context}", error.Message));
            }
        }

        rows.Add(("Build", ViewModel.BuildLabelText.StartsWith("Build ", StringComparison.Ordinal)
            ? ViewModel.BuildLabelText["Build ".Length..]
            : ViewModel.BuildLabelText));
        rows.Add(("Logs", DisplayOrPlaceholder(status?.LogDirectory)));
        return rows;
    }

    private static string SkipReasonLabel(string? reason) =>
        reason switch
        {
            null or "" => "<none>",
            "static_screen" => "Screen unchanged since the last upload",
            "locked_or_screensaver" => "Screen locked or screensaver active",
            "capture_failed" => "Capture failed",
            _ => reason,
        };

    // ---- Actions -----------------------------------------------------------

    private async Task ForceCaptureAsync()
    {
        var result = await ViewModel.ForceCaptureAsync();
        if (result is not null)
        {
            // The interop call waited for the batch, so `result.Message` says
            // what really happened: uploaded, gated, or still in flight.
            await ShowMessageAsync(result.Outcome == "uploaded" ? "Screenshot Uploaded" : "Test Screenshot", result.Message);
        }
    }

    private async Task SendReportAsync()
    {
        var message = _reportMessageBox.Text.Trim();
        if (string.IsNullOrEmpty(message))
        {
            SetError(_reportError, "Please describe the issue.");
            _reportMessageBox.Focus(FocusState.Programmatic);
            return;
        }

        SetError(_reportError, null);
        var contactEmail = _reportEmailBox.Text.Trim();
        var succeeded = await ViewModel.SubmitBugReportAsync(
            message,
            string.IsNullOrEmpty(contactEmail) ? null : contactEmail,
            _reportLogsCheckBox.IsChecked == true);

        if (!succeeded)
        {
            SetError(_reportError, ViewModel.ErrorText ?? "Failed to send the report.");
            return;
        }

        await ShowMessageAsync("Report Sent", "Thanks, your report was sent to the Virtue Initiative team.");
        Navigate(Screen.Home);
    }

    public async Task<bool> ShowStopMonitoringConfirmationAsync()
    {
        return await ConfirmAsync(
            "Stop monitoring and close Virtue?",
            "This will stop monitoring on this device and close the main window and tray app.",
            "People monitoring you may be alerted.",
            "Stop Monitoring");
    }

    private async Task<bool> ShowLogoutConfirmationAsync()
    {
        return await ConfirmAsync(
            "Sign out of Virtue?",
            "Signing out will deactivate this device and stop monitoring. Logging in again will create a new device.",
            "Anyone monitoring you may be alerted.",
            "Sign Out");
    }

    /// <summary>
    /// A destructive confirmation: the action button is red and nothing is the
    /// default, so Enter can't confirm by accident and Esc cancels.
    /// </summary>
    private async Task<bool> ConfirmAsync(string title, string body, string warning, string action)
    {
        var content = new StackPanel { Spacing = 8, MaxWidth = 420 };
        content.Children.Add(new TextBlock { Text = body, TextWrapping = TextWrapping.Wrap, Foreground = TextBrush });
        content.Children.Add(new TextBlock { Text = warning, TextWrapping = TextWrapping.Wrap, Foreground = TextBrush, FontWeight = FontWeights.SemiBold });

        var dialog = new ContentDialog
        {
            Title = title,
            Content = content,
            PrimaryButtonText = action,
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.None,
        };
        ApplyDialogTheme(dialog, (BadBrush, BadHoverBrush, BadHoverBrush));
        dialog.PrimaryButtonStyle = (Style)Application.Current.Resources["AccentButtonStyle"];
        var result = await ShowDialogAsync(dialog);
        return result == ContentDialogResult.Primary;
    }

    private async Task ShowMessageAsync(string title, string message)
    {
        var dialog = new ContentDialog
        {
            Title = title,
            Content = new TextBlock { Text = message, TextWrapping = TextWrapping.Wrap, Foreground = TextBrush, MaxWidth = 380 },
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
        };
        ApplyDialogTheme(dialog, (AccentBrush, AccentHoverBrush, AccentPressedBrush));
        await ShowDialogAsync(dialog);
    }

    private async Task<ContentDialogResult> ShowDialogAsync(ContentDialog dialog)
    {
        if (Content is FrameworkElement root)
        {
            dialog.XamlRoot = root.XamlRoot;
            dialog.RequestedTheme = ElementTheme.Light;
        }

        return await dialog.ShowAsync();
    }

    private static void ApplyDialogTheme(ContentDialog dialog, (SolidColorBrush Normal, SolidColorBrush Hover, SolidColorBrush Pressed) accent)
    {
        var r = dialog.Resources;
        r["ContentDialogBackground"] = BgBrush;
        r["ContentDialogTopOverlay"] = BgBrush;
        r["ContentDialogForeground"] = TextBrush;
        r["ContentDialogBorderBrush"] = BorderBrushToken;
        r["ContentDialogSeparatorBorderBrush"] = BorderBrushToken;

        r["AccentButtonBackground"] = accent.Normal;
        r["AccentButtonBackgroundPointerOver"] = accent.Hover;
        r["AccentButtonBackgroundPressed"] = accent.Pressed;
        r["AccentButtonForeground"] = OnAccentBrush;
        r["AccentButtonForegroundPointerOver"] = OnAccentBrush;
        r["AccentButtonForegroundPressed"] = OnAccentBrush;
        r["AccentButtonBorderBrush"] = accent.Normal;
        r["AccentButtonBorderBrushPointerOver"] = accent.Hover;
        r["AccentButtonBorderBrushPressed"] = accent.Pressed;

        r["ButtonBackground"] = SurfaceBrush;
        r["ButtonBackgroundPointerOver"] = BgSubtleBrush;
        r["ButtonBackgroundPressed"] = BgSubtleBrush;
        r["ButtonForeground"] = TextBrush;
        r["ButtonForegroundPointerOver"] = TextBrush;
        r["ButtonForegroundPressed"] = TextBrush;
        r["ButtonBorderBrush"] = BorderBrushToken;
        r["ButtonBorderBrushPointerOver"] = BorderHoverBrush;
        r["ButtonBorderBrushPressed"] = BorderHoverBrush;
    }

    // ---- Building blocks ---------------------------------------------------

    private static UIElement CreateColumn(UIElement content) =>
        new Border
        {
            Child = content,
            MaxWidth = 448,
            Padding = new Thickness(24, 20, 24, 28),
            VerticalAlignment = VerticalAlignment.Top,
        };

    private static Border CreateCard(UIElement content) =>
        new()
        {
            Background = SurfaceBrush,
            CornerRadius = new CornerRadius(8),
            BorderBrush = BorderBrushToken,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(16),
            Child = content,
        };

    private static FrameworkElement CreateSection(string title, IReadOnlyList<UIElement> rows)
    {
        var section = new StackPanel { Spacing = 10 };
        var heading = new TextBlock
        {
            Text = title,
            FontSize = 15,
            FontWeight = FontWeights.SemiBold,
            Foreground = TextBrush,
            Margin = new Thickness(2, 0, 0, 0),
        };
        AutomationProperties.SetHeadingLevel(heading, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level2);
        section.Children.Add(heading);
        section.Children.Add(CreateGroup(rows));
        return section;
    }

    private static Border CreateGroup(IReadOnlyList<UIElement> rows)
    {
        var stack = new StackPanel();
        for (var i = 0; i < rows.Count; i++)
        {
            if (i > 0)
            {
                stack.Children.Add(new Rectangle { Height = 1, Fill = BorderBrushToken });
            }
            stack.Children.Add(rows[i]);
        }

        return new Border
        {
            Background = SurfaceBrush,
            CornerRadius = new CornerRadius(8),
            BorderBrush = BorderBrushToken,
            BorderThickness = new Thickness(1),
            Child = stack,
        };
    }

    /// <summary>
    /// Dot, muted label, right-aligned value, and a hint underneath only when
    /// the user must act. Long values wrap in the space the label leaves.
    /// </summary>
    private static Grid CreateRow(StatusKind? kind, string label, string value, string? hint = null)
    {
        var row = new Grid { Padding = new Thickness(14), ColumnSpacing = 12, RowSpacing = 4 };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        row.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        if (kind is { } k)
        {
            var dot = CreateDot(k, 10);
            dot.VerticalAlignment = VerticalAlignment.Top;
            dot.Margin = new Thickness(0, 5, 0, 0);
            row.Children.Add(dot);
        }
        else
        {
            row.ColumnSpacing = 0;
        }

        var labelBlock = new TextBlock
        {
            Text = label,
            FontSize = 13,
            Foreground = TextMutedBrush,
            Margin = new Thickness(0, 1, kind is null ? 12 : 0, 0),
            MaxWidth = 240,
            TextWrapping = TextWrapping.Wrap,
        };
        Grid.SetColumn(labelBlock, 1);
        row.Children.Add(labelBlock);

        var valueBlock = new TextBlock
        {
            Text = value,
            FontSize = 14,
            FontWeight = FontWeights.SemiBold,
            Foreground = kind switch
            {
                StatusKind.Bad => BadBrush,
                StatusKind.Warn => LinkBrush,
                _ => TextBrush,
            },
            TextAlignment = TextAlignment.Right,
            HorizontalAlignment = HorizontalAlignment.Right,
            TextWrapping = TextWrapping.Wrap,
            IsTextSelectionEnabled = true,
        };
        Grid.SetColumn(valueBlock, 2);
        row.Children.Add(valueBlock);

        if (hint is not null)
        {
            var hintBlock = new TextBlock
            {
                Text = hint,
                FontSize = 12,
                Foreground = TextMutedBrush,
                TextWrapping = TextWrapping.Wrap,
                IsTextSelectionEnabled = true,
            };
            Grid.SetRow(hintBlock, 1);
            Grid.SetColumn(hintBlock, 1);
            Grid.SetColumnSpan(hintBlock, 2);
            row.Children.Add(hintBlock);
        }

        AutomationProperties.SetName(row, hint is null ? $"{label}: {value}" : $"{label}: {value}. {hint}");
        return row;
    }

    private static Ellipse CreateDot(StatusKind kind, double size)
    {
        var dot = new Ellipse { Width = size, Height = size, Fill = KindBrush(kind), VerticalAlignment = VerticalAlignment.Center };
        AutomationProperties.SetName(dot, kind switch
        {
            StatusKind.Good => "Working",
            StatusKind.Warn => "Waiting",
            StatusKind.Bad => "Needs attention",
            _ => "Not applicable",
        });
        return dot;
    }

    private static SolidColorBrush KindBrush(StatusKind kind) =>
        kind switch
        {
            StatusKind.Good => GoodBrush,
            StatusKind.Warn => WarnBrush,
            StatusKind.Bad => BadBrush,
            _ => NeutralBrush,
        };

    /// <summary>A stroked glyph drawn on a 24-unit grid, scaled to <paramref name="size"/>.</summary>
    private static Viewbox CreateGlyph(string data, Brush brush, double size, double strokeThickness)
    {
        var canvas = new Grid { Width = 24, Height = 24 };
        canvas.Children.Add(new Path
        {
            Data = (Geometry)XamlBindingHelper.ConvertValue(typeof(Geometry), data),
            Stroke = brush,
            StrokeThickness = strokeThickness,
            StrokeStartLineCap = PenLineCap.Round,
            StrokeEndLineCap = PenLineCap.Round,
            StrokeLineJoin = PenLineJoin.Round,
        });
        return new Viewbox { Width = size, Height = size, Child = canvas };
    }

    private static Viewbox CreateExclamation(Brush brush)
    {
        var box = CreateGlyph("M12,6 L12,14", brush, 52, 2);
        if (box.Child is Grid canvas)
        {
            canvas.Children.Add(new Ellipse { Width = 2.2, Height = 2.2, Fill = brush, Margin = new Thickness(10.9, 16.9, 0, 0), HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top });
        }
        return box;
    }

    private static Viewbox CreateThreeDots(Brush brush)
    {
        var canvas = new Grid { Width = 24, Height = 24 };
        foreach (var x in new[] { 6.0, 12.0, 18.0 })
        {
            canvas.Children.Add(new Ellipse
            {
                Width = 3.6,
                Height = 3.6,
                Fill = brush,
                HorizontalAlignment = HorizontalAlignment.Left,
                VerticalAlignment = VerticalAlignment.Top,
                Margin = new Thickness(x - 1.8, 12 - 1.8, 0, 0),
            });
        }
        return new Viewbox { Width = 52, Height = 52, Child = canvas };
    }

    /// <summary>
    /// The V from <c>landing/public/logo.svg</c>, translated so its viewBox is
    /// cropped to <c>3 6 54 58</c>.
    /// </summary>
    private static Viewbox CreateLogo(double height)
    {
        var canvas = new Grid { Width = 54, Height = 58 };
        foreach (var data in new[]
                 {
                     "M0.875342,1.45582 l19.617861,46.49865 6.207601,-14.75956 -12.019272,-31.74121 z",
                     "M43.380729,1.47666 l-21.288034,49.2949 c0,0 6.922226,8.9725 12.372314,-4.64125 6.834393,-17.43578 13.178419,-27.2295 18.632133,-44.65905 z",
                 })
        {
            canvas.Children.Add(new Path
            {
                Data = (Geometry)XamlBindingHelper.ConvertValue(typeof(Geometry), data),
                Fill = LogoBrush,
                Stroke = LogoBrush,
                StrokeThickness = 0.388117,
                StrokeLineJoin = PenLineJoin.Round,
            });
        }

        var box = new Viewbox { Height = height, Child = canvas };
        AutomationProperties.SetAccessibilityView(box, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
        return box;
    }

    private static MenuFlyoutItem CreateMenuItem(string text, string glyph)
    {
        var item = new MenuFlyoutItem { Text = text, Icon = new FontIcon { Glyph = glyph } };
        item.Resources["MenuFlyoutItemBackgroundPointerOver"] = BgSubtleBrush;
        item.Resources["MenuFlyoutItemBackgroundPressed"] = BorderBrushToken;
        return item;
    }

    private static MenuFlyoutItem CreateMenuNote(string text)
    {
        var item = new MenuFlyoutItem { Text = text, IsEnabled = false, FontSize = 12 };
        item.Resources["MenuFlyoutItemForegroundDisabled"] = TextMutedBrush;
        return item;
    }

    private static Style CreateMenuPresenterStyle()
    {
        var style = new Style(typeof(MenuFlyoutPresenter));
        style.Setters.Add(new Setter(Control.BackgroundProperty, SurfaceBrush));
        style.Setters.Add(new Setter(Control.BorderBrushProperty, BorderBrushToken));
        style.Setters.Add(new Setter(FrameworkElement.MinWidthProperty, 240d));
        return style;
    }

    private static HyperlinkButton CreateLink(string text, string url)
    {
        var link = new HyperlinkButton
        {
            Content = text,
            NavigateUri = new Uri(url),
            HorizontalAlignment = HorizontalAlignment.Left,
            Padding = new Thickness(0, 4, 0, 4),
            Foreground = LinkBrush,
        };
        link.Resources["HyperlinkButtonForeground"] = LinkBrush;
        link.Resources["HyperlinkButtonForegroundPointerOver"] = LinkBrush;
        link.Resources["HyperlinkButtonForegroundPressed"] = LinkBrush;
        link.Resources["HyperlinkButtonBackgroundPointerOver"] = new SolidColorBrush(Colors.Transparent);
        link.Resources["HyperlinkButtonBackgroundPressed"] = new SolidColorBrush(Colors.Transparent);
        return link;
    }

    private static InfoBar CreateErrorBar() =>
        new()
        {
            Severity = InfoBarSeverity.Error,
            IsClosable = false,
            IsOpen = false,
        };

    private static void SetError(InfoBar bar, string? message)
    {
        bar.Message = message ?? string.Empty;
        bar.IsOpen = !string.IsNullOrWhiteSpace(message);
    }

    // Filled accent action (the primary button on each screen).
    private static Button CreatePrimaryButton(string text)
    {
        var button = new Button
        {
            Content = text,
            Padding = new Thickness(20, 8, 20, 9),
            FontSize = 14,
        };
        var r = button.Resources;
        r["ButtonBackground"] = AccentBrush;
        r["ButtonBackgroundPointerOver"] = AccentHoverBrush;
        r["ButtonBackgroundPressed"] = AccentPressedBrush;
        r["ButtonBackgroundDisabled"] = BorderHoverBrush;
        r["ButtonForeground"] = OnAccentBrush;
        r["ButtonForegroundPointerOver"] = OnAccentBrush;
        r["ButtonForegroundPressed"] = OnAccentBrush;
        r["ButtonForegroundDisabled"] = OnAccentBrush;
        r["ButtonBorderBrush"] = AccentBrush;
        r["ButtonBorderBrushPointerOver"] = AccentHoverBrush;
        r["ButtonBorderBrushPressed"] = AccentPressedBrush;
        r["ButtonBorderBrushDisabled"] = BorderHoverBrush;
        return button;
    }

    // Surface-filled secondary action on a hairline border.
    private static Button CreateSecondaryButton(string text)
    {
        var button = new Button
        {
            Content = text,
            Padding = new Thickness(20, 8, 20, 9),
            FontSize = 14,
        };
        var r = button.Resources;
        r["ButtonBackground"] = SurfaceBrush;
        r["ButtonBackgroundPointerOver"] = BgSubtleBrush;
        r["ButtonBackgroundPressed"] = BgSubtleBrush;
        r["ButtonForeground"] = TextBrush;
        r["ButtonForegroundPointerOver"] = TextBrush;
        r["ButtonForegroundPressed"] = TextBrush;
        r["ButtonBorderBrush"] = BorderBrushToken;
        r["ButtonBorderBrushPointerOver"] = BorderHoverBrush;
        r["ButtonBorderBrushPressed"] = BorderHoverBrush;
        return button;
    }

    private static Button CreateIconButton(string glyph, string name, double width, double height)
    {
        var button = new Button { Content = new FontIcon { Glyph = glyph, FontSize = 14, Foreground = TextBrush } };
        StyleIconButton(button, width, height);
        AutomationProperties.SetName(button, name);
        ToolTipService.SetToolTip(button, name);
        return button;
    }

    // Borderless until hovered: the menu, back and reveal buttons.
    private static void StyleIconButton(Button button, double width, double height)
    {
        button.Width = width;
        button.Height = height;
        button.Padding = new Thickness(0);
        StyleQuietButton(button);
    }

    private static void StyleQuietButton(Button button)
    {
        var transparent = new SolidColorBrush(Colors.Transparent);
        button.Background = transparent;
        button.BorderThickness = new Thickness(0);
        var r = button.Resources;
        r["ButtonBackground"] = transparent;
        r["ButtonBackgroundPointerOver"] = BgSubtleBrush;
        r["ButtonBackgroundPressed"] = BorderBrushToken;
        r["ButtonBorderBrush"] = transparent;
        r["ButtonBorderBrushPointerOver"] = transparent;
        r["ButtonBorderBrushPressed"] = transparent;
    }

    // Surface-filled input on a hairline border; the focus line goes accent.
    private static void StyleInput(Control input)
    {
        input.FontSize = 14;
        input.MinHeight = 36;
        var r = input.Resources;
        r["TextControlBackground"] = SurfaceBrush;
        r["TextControlBackgroundPointerOver"] = SurfaceBrush;
        r["TextControlBackgroundFocused"] = SurfaceBrush;
        r["TextControlBorderBrush"] = BorderHoverBrush;
        r["TextControlBorderBrushPointerOver"] = TextMutedBrush;
        r["TextControlBorderBrushFocused"] = AccentBrush;
        r["TextControlForeground"] = TextBrush;
        r["TextControlForegroundPointerOver"] = TextBrush;
        r["TextControlForegroundFocused"] = TextBrush;
        r["TextControlPlaceholderForeground"] = TextMutedBrush;
        r["TextControlPlaceholderForegroundPointerOver"] = TextMutedBrush;
        r["TextControlPlaceholderForegroundFocused"] = TextMutedBrush;
    }

    // The CheckBox template's visual states set the label color from theme
    // resources, overriding Foreground, so pin every state to the text color
    // with an accent-filled box when checked.
    private static void StyleCheckBox(CheckBox checkBox)
    {
        var r = checkBox.Resources;
        foreach (var state in new[] { "Unchecked", "Checked", "Indeterminate" })
        {
            foreach (var suffix in new[] { "", "PointerOver", "Pressed" })
            {
                r[$"CheckBoxForeground{state}{suffix}"] = TextBrush;
            }
        }

        foreach (var suffix in new[] { "", "PointerOver", "Pressed" })
        {
            r[$"CheckBoxCheckBackgroundFillChecked{suffix}"] = AccentBrush;
            r[$"CheckBoxCheckBackgroundStrokeChecked{suffix}"] = AccentBrush;
            r[$"CheckBoxCheckGlyphForegroundChecked{suffix}"] = OnAccentBrush;
            r[$"CheckBoxCheckBackgroundStrokeUnchecked{suffix}"] = TextMutedBrush;
        }
    }

    private static Visibility VisibleIf(bool visible) => visible ? Visibility.Visible : Visibility.Collapsed;

    // ---- Window plumbing ---------------------------------------------------

    private void SetWindowIcon()
    {
        var iconCandidates = new[]
        {
            System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "app-icon.ico"),
            System.IO.Path.Combine(AppContext.BaseDirectory, "app-icon.ico"),
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

    /// <summary>
    /// Sizes the window from effective pixels so it opens at the same visual
    /// size at any display scaling, and centers it in the work area of the
    /// monitor it opens on (see <see cref="InitialWindowBounds"/>).
    /// </summary>
    private void SetInitialBounds()
    {
        var dpi = GetDpiForWindow(WindowNative.GetWindowHandle(this));
        var workArea = DisplayArea.GetFromWindowId(_appWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        var bounds = InitialWindowBounds.Compute(
            dpi,
            new PixelRect(workArea.X, workArea.Y, workArea.Width, workArea.Height));
        _appWindow.MoveAndResize(new RectInt32(bounds.X, bounds.Y, bounds.Width, bounds.Height));
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
            // Nothing actionable for the user here — the path is listed under
            // Advanced either way, so a failed launch isn't worth an error dialog.
            System.Diagnostics.Debug.WriteLine($"failed to open log folder: {ex}");
        }
    }

    private static void OpenInBrowser(string url)
    {
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = url,
                UseShellExecute = true,
            });
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"failed to open {url}: {ex}");
        }
    }

    private static string DisplayOrPlaceholder(string? value) =>
        string.IsNullOrWhiteSpace(value) ? "<none>" : value;

    /// <summary>Local time with a relative age (CLIENT-007).</summary>
    private static string FormatTimestamp(long? value)
    {
        if (!value.HasValue)
        {
            return "<none>";
        }

        var at = DateTimeOffset.FromUnixTimeMilliseconds(value.Value);
        var age = DateTimeOffset.UtcNow - at;
        var ageText = age.TotalSeconds < 0
            ? "in the future"
            : age.TotalSeconds < 60 ? $"{(int)age.TotalSeconds}s ago"
            : age.TotalMinutes < 60 ? $"{(int)age.TotalMinutes}m ago"
            : age.TotalHours < 24 ? $"{(int)age.TotalHours}h ago"
            : $"{(int)age.TotalDays}d ago";
        return $"{at.ToLocalTime():yyyy-MM-dd HH:mm:ss} ({ageText})";
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
    private static extern uint GetDpiForWindow(IntPtr hWnd);

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
