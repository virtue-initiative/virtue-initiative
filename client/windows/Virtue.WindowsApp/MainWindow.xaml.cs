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
    private const string WebsiteDisplayUrl = "virtue-initiative.org";
    private const string WebsiteNavigateUrl = "https://virtueinitiative.org";

    private readonly AppWindow _appWindow;
    private readonly TextBlock _statusTextBlock;
    private readonly TextBlock _statusDetailTextBlock;
    private readonly TextBlock _buildLabelTextBlock;
    private readonly TextBlock _accountSummaryTextBlock;
    private readonly StackPanel _loginPanel;
    private readonly StackPanel _signedInActionsPanel;
    private readonly TextBox _emailTextBox;
    private readonly PasswordBox _passwordBox;
    private bool _allowClose;

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
        _loginPanel = new StackPanel();
        _signedInActionsPanel = new StackPanel();

        Content = BuildContent();
        ViewModel.PropertyChanged += ViewModelOnPropertyChanged;
        _appWindow = ResolveAppWindow();
        _appWindow.Closing += AppWindowOnClosing;
        _appWindow.Resize(new SizeInt32(720, 560));
        SetWindowIcon();
        SyncFromViewModel();
    }

    public SessionViewModel ViewModel { get; }

    public void HideToTray()
    {
        var windowHandle = WindowNative.GetWindowHandle(this);
        ShowWindow(windowHandle, ShowWindowCommands.Hide);
    }

    public void ShowFromTray()
    {
        var windowHandle = WindowNative.GetWindowHandle(this);
        ShowWindow(windowHandle, ShowWindowCommands.Restore);
        Activate();
        SetForegroundWindow(windowHandle);
    }

    public void PrepareForExit()
    {
        _allowClose = true;
    }

    private UIElement BuildContent()
    {
        _emailTextBox.PlaceholderText = "Email";
        _emailTextBox.TextChanged += (_, _) => ViewModel.EmailInput = _emailTextBox.Text;

        _passwordBox.PlaceholderText = "Password";
        _passwordBox.PasswordRevealMode = PasswordRevealMode.Hidden;
        _passwordBox.PasswordChanged += PasswordBox_OnPasswordChanged;

        var root = new Grid
        {
            Background = new LinearGradientBrush
            {
                StartPoint = new Windows.Foundation.Point(0, 0),
                EndPoint = new Windows.Foundation.Point(1, 1),
                GradientStops =
                {
                    new GradientStop { Color = ColorFromHex("#F7FBFF"), Offset = 0.0 },
                    new GradientStop { Color = ColorFromHex("#EEF5F2"), Offset = 1.0 },
                },
            },
        };

        var contentStack = new StackPanel
        {
            Spacing = 20,
            Padding = new Thickness(28, 30, 28, 28),
            // MaxWidth = 680,
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
            FontSize = 30,
            FontWeight = FontWeights.SemiBold,
        });
        textStack.Children.Add(new TextBlock
        {
            Text = "Private, resident monitoring on this device.",
            Foreground = new SolidColorBrush(ColorFromHex("#4B5E68")),
        });
        textStack.Children.Add(new HyperlinkButton
        {
            Content = WebsiteDisplayUrl,
            NavigateUri = new Uri(WebsiteNavigateUrl),
            HorizontalAlignment = HorizontalAlignment.Left,
            Padding = new Thickness(0),
        });

        return header;
    }

    private UIElement BuildStatusCard()
    {
        _statusTextBlock.FontSize = 24;
        _statusTextBlock.FontWeight = FontWeights.SemiBold;
        _statusTextBlock.Foreground = new SolidColorBrush(ColorFromHex("#133043"));

        _statusDetailTextBlock.Foreground = new SolidColorBrush(ColorFromHex("#4B5E68"));
        _buildLabelTextBlock.Foreground = new SolidColorBrush(ColorFromHex("#60727C"));

        var detailsButton = CreateActionButton("Status Details");
        detailsButton.Click += StatusDetailsButton_OnClick;

        var stopMonitoringButton = CreateActionButton("Stop Monitoring");
        stopMonitoringButton.Click += async (_, _) => await ViewModel.StopMonitoringAsync();

        var signOutButton = CreateActionButton("Sign Out");
        signOutButton.Click += async (_, _) => await ViewModel.LogoutAsync();

        _signedInActionsPanel.Orientation = Orientation.Horizontal;
        _signedInActionsPanel.Spacing = 10;
        _signedInActionsPanel.Children.Add(stopMonitoringButton);
        _signedInActionsPanel.Children.Add(signOutButton);

        var actionRow = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 10,
            Margin = new Thickness(0, 18, 0, 0),
        };
        actionRow.Children.Add(detailsButton);
        actionRow.Children.Add(_signedInActionsPanel);

        var content = new StackPanel { Spacing = 8 };
        content.Children.Add(CreateSectionLabel("Status"));
        content.Children.Add(_statusTextBlock);
        content.Children.Add(_statusDetailTextBlock);
        content.Children.Add(_buildLabelTextBlock);
        content.Children.Add(actionRow);

        return CreateCard(content);
    }

    private UIElement BuildAccountCard()
    {
        _accountSummaryTextBlock.FontSize = 18;
        _accountSummaryTextBlock.Foreground = new SolidColorBrush(ColorFromHex("#133043"));

        _loginPanel.Spacing = 12;
        _loginPanel.Margin = new Thickness(0, 12, 0, 0);
        _loginPanel.Children.Add(_emailTextBox);
        _loginPanel.Children.Add(_passwordBox);

        var signInButton = CreatePrimaryButton("Sign In");
        signInButton.Click += SignInButton_OnClick;
        _loginPanel.Children.Add(signInButton);

        var content = new StackPanel { Spacing = 8 };
        content.Children.Add(CreateSectionLabel("Account"));
        content.Children.Add(_accountSummaryTextBlock);
        content.Children.Add(_loginPanel);

        return CreateCard(content);
    }

    private Border CreateCard(UIElement content) =>
        new()
        {
            Background = new SolidColorBrush(Colors.White),
            CornerRadius = new CornerRadius(18),
            BorderBrush = new SolidColorBrush(ColorFromHex("#D8E3E0")),
            BorderThickness = new Thickness(1),
            Padding = new Thickness(20),
            Shadow = new ThemeShadow(),
            Child = content,
        };

    private static TextBlock CreateSectionLabel(string text) =>
        new()
        {
            Text = text,
            FontSize = 13,
            FontWeight = FontWeights.Medium,
            Foreground = new SolidColorBrush(ColorFromHex("#60727C")),
        };

    private static Button CreatePrimaryButton(string text) =>
        new()
        {
            Content = text,
            HorizontalAlignment = HorizontalAlignment.Left,
            MinWidth = 120,
        };

    private static Button CreateActionButton(string text) =>
        new()
        {
            Content = text,
            HorizontalAlignment = HorizontalAlignment.Left,
        };

    private async void StatusDetailsButton_OnClick(object sender, RoutedEventArgs e)
    {
        await ViewModel.RefreshAsync();
        await ShowStatusDialogAsync();
    }

    private async Task ShowStatusDialogAsync()
    {
        var statusBox = new TextBox
        {
            Text = BuildStatusDetailsText(),
            IsReadOnly = true,
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            MinWidth = 520,
            MinHeight = 320,
            BorderThickness = new Thickness(0),
            Background = new SolidColorBrush(Colors.Transparent),
            FontFamily = new FontFamily("Consolas"),
        };

        var dialog = new ContentDialog
        {
            Title = "Virtue Status",
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
            Content = new ScrollViewer
            {
                Content = statusBox,
                VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            },
        };

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

        lines.Add($"capture_interval_seconds: {ViewModel.CaptureIntervalSeconds}");
        lines.Add($"batch_window_seconds: {ViewModel.BatchWindowSeconds}");
        lines.Add($"base_api_url: {ViewModel.ApiBaseUrl}");

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
        _statusDetailTextBlock.Text = BuildSecondaryStatusText();
        _buildLabelTextBlock.Text = ViewModel.BuildLabelText;
        _accountSummaryTextBlock.Text = ViewModel.LoggedIn
            ? $"Signed in as {ViewModel.AccountSummary}"
            : "Sign in to start monitoring.";
        _loginPanel.Visibility = ViewModel.LoggedIn ? Visibility.Collapsed : Visibility.Visible;
        _signedInActionsPanel.Visibility = ViewModel.LoggedIn ? Visibility.Visible : Visibility.Collapsed;

        if (_emailTextBox.Text != ViewModel.EmailInput)
        {
            _emailTextBox.Text = ViewModel.EmailInput;
        }

        if (_passwordBox.Password != ViewModel.PasswordInput)
        {
            _passwordBox.Password = ViewModel.PasswordInput;
        }
    }

    private string BuildPrimaryStatusText() =>
        ViewModel.MonitorState switch
        {
            "running" => "Monitoring active",
            "starting" => "Starting monitoring",
            "signed_out" => "Logged out",
            "error" => "Attention needed",
            _ => "Monitoring stopped",
        };

    private string BuildSecondaryStatusText()
    {
        if (!string.IsNullOrWhiteSpace(ViewModel.MonitorError))
        {
            return ViewModel.MonitorError!;
        }

        return ViewModel.MonitorState switch
        {
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
