## Android installation

Virtue isn't in the Play Store yet, so you install it from a downloaded file. These steps use Chrome, and other browsers work the same way.

1. If you do not have one, [create an account](/signup).
2. On your phone, download the [Android `.apk` file]({ANDROID_DOWNLOAD}).
3. If Chrome says **File might be harmful**, tap **Download anyway**.
   ![Chrome's "File might be harmful" warning, with "Download anyway" highlighted](/images/android-setup/download-warning.webp)
4. Tap **Open** when the download finishes. If you missed it, open Chrome's menu and tap **Downloads**, then tap the file.
5. If Android says your phone isn't allowed to install unknown apps from Chrome, tap **Settings**.
   ![Android's warning about installing unknown apps, with "Settings" highlighted](/images/android-setup/install-blocked.webp)
6. Turn on **Allow from this source**, then go back.
   ![The "Install unknown apps" screen for Chrome, with "Allow from this source" highlighted](/images/android-setup/install-allow-source.webp)
7. Tap **Install**, then tap **Open**.
   ![Android asking "Do you want to install this app?", with "Install" highlighted](/images/android-setup/install-confirm.webp)
8. Tap **Allow** when Virtue asks to run in the background and to send notifications.
9. Turn on Accessibility for Virtue using the steps below. The app shows the same steps with screenshots for your phone.

<div class="android-guide" data-android-guide>
<label class="android-guide-version">
<span>Your Android version</span>
<select data-android-version>
<option value="17">Android 17</option>
<option value="16" selected>Android 16</option>
<option value="15">Android 15</option>
<option value="14">Android 14</option>
<option value="13">Android 13</option>
<option value="12">Android 12</option>
<option value="10">Android 10 or 11</option>
</select>
</label>
<p class="android-guide-hint">To check your version, open <strong>Settings</strong>, tap <strong>About phone</strong>, and look for <strong>Android version</strong>.</p>
<p data-min="13">Android blocks Accessibility access for apps installed from a downloaded file. The first three steps unlock it.</p>
<ol>
<li>
Open <strong>Settings</strong>, then <strong>Accessibility</strong>. You can also tap <strong>Open Accessibility Settings</strong> in the Virtue app. Under <strong><span data-max="10">Downloaded services</span><span data-min="12">Downloaded apps</span></strong>, tap <strong>Virtue Screen Monitor</strong>.<span data-min="13"> It is grayed out, which is expected.</span>
<img data-max="10" src="/images/android-setup/a11y_list_v10.webp" alt="Accessibility settings on Android 10, with Virtue Screen Monitor highlighted under Downloaded services">
<img data-min="12" data-max="12" src="/images/android-setup/a11y_list_v12.webp" alt="Accessibility settings on Android 12, with Virtue Screen Monitor highlighted under Downloaded apps">
<img data-min="13" data-max="14" src="/images/android-setup/a11y_list_v13.webp" alt="Accessibility settings on Android 13, with the grayed-out Virtue Screen Monitor highlighted">
<img data-min="15" data-max="16" src="/images/android-setup/a11y_list_v15.webp" alt="Accessibility settings on Android 15, with Virtue Screen Monitor highlighted and marked Controlled by Restricted Setting">
<img data-min="17" src="/images/android-setup/a11y_list_v17.webp" alt="Accessibility settings on Android 17, with Virtue Screen Monitor highlighted and marked Controlled by Restricted Setting">
</li>
<li data-min="13">
Android shows <strong><span data-max="14">Restricted setting</span><span data-min="15">App was denied access</span></strong>. Tap <strong><span data-max="15">OK</span><span data-min="16">Close</span></strong>.
<img data-min="13" data-max="14" src="/images/android-setup/a11y_restricted_v13.webp" alt="The Restricted setting message, with OK highlighted">
<img data-min="15" data-max="15" src="/images/android-setup/a11y_restricted_v15.webp" alt="The App was denied access message, with OK highlighted">
<img data-min="16" data-max="16" src="/images/android-setup/a11y_restricted_v16.webp" alt="The App was denied access message, with Close highlighted">
<img data-min="17" src="/images/android-setup/a11y_restricted_v17.webp" alt="The App was denied access message on Android 17, with Close highlighted">
</li>
<li data-min="13">
<span data-max="13">In the Virtue app, tap <strong>Open App List</strong>, then tap <strong>Virtue</strong>. You can also open <strong>Settings</strong>, tap <strong>Apps</strong>, and find Virtue in the list of all apps.</span><span data-min="14">In the Virtue app, tap <strong>Open App Info</strong>. You can also open <strong>Settings</strong>, tap <strong>Apps</strong>, and tap <strong>Virtue</strong>.</span> On the App info screen, tap <strong>⋮</strong> in the top corner, then tap <strong>Allow restricted settings</strong>. Enter your PIN if Android asks for it.
<img data-min="13" data-max="14" src="/images/android-setup/a11y_menu_v13.webp" alt="Virtue's App info screen with the menu open and Allow restricted settings highlighted">
<img data-min="15" data-max="16" src="/images/android-setup/a11y_menu_v15.webp" alt="Virtue's App info screen on Android 15 with Allow restricted settings highlighted">
<img data-min="17" src="/images/android-setup/a11y_menu_v17.webp" alt="Virtue's App info screen on Android 17 with Allow restricted settings highlighted">
</li>
<li>
<span data-min="13">Go back to <strong>Accessibility</strong> and tap <strong>Virtue Screen Monitor</strong> again. </span>Turn on <strong><span data-max="10">Use service</span><span data-min="12">Use Virtue Screen Monitor</span></strong>.
<img data-max="10" src="/images/android-setup/a11y_toggle_v10.webp" alt="The Virtue Screen Monitor screen on Android 10, with Use service highlighted">
<img data-min="12" data-max="12" src="/images/android-setup/a11y_toggle_v12.webp" alt="The Virtue Screen Monitor screen on Android 12, with Use Virtue Screen Monitor highlighted">
<img data-min="13" data-max="14" src="/images/android-setup/a11y_toggle_v13.webp" alt="The Virtue Screen Monitor screen on Android 13, with Use Virtue Screen Monitor highlighted">
<img data-min="15" data-max="16" src="/images/android-setup/a11y_toggle_v15.webp" alt="The Virtue Screen Monitor screen on Android 15, with Use Virtue Screen Monitor highlighted">
<img data-min="17" src="/images/android-setup/a11y_toggle_v17.webp" alt="The Virtue Screen Monitor screen on Android 17, with Use Virtue Screen Monitor highlighted">
</li>
<li>
Tap <strong>Allow</strong> to let Virtue capture screenshots.
<img data-max="10" src="/images/android-setup/a11y_allow_v10.webp" alt="The full control confirmation on Android 10, with Allow highlighted">
<img data-min="12" data-max="12" src="/images/android-setup/a11y_allow_v12.webp" alt="The full control confirmation on Android 12, with Allow highlighted">
<img data-min="13" data-max="14" src="/images/android-setup/a11y_allow_v13.webp" alt="The full control confirmation on Android 13, with Allow highlighted">
<img data-min="15" data-max="16" src="/images/android-setup/a11y_allow_v15.webp" alt="The full control confirmation on Android 15, with Allow highlighted">
<img data-min="17" src="/images/android-setup/a11y_allow_v17.webp" alt="The full control confirmation on Android 17, with Allow highlighted">
</li>
</ol>
</div>

10. Go back to the Virtue app and sign in with your account.
11. Done! The app will periodically collect screenshots (about once every 5 minutes) and upload them once an hour. You and your partners will be able to view them from the logs page on the website.

## Updates

Virtue checks for new versions on its own and downloads them over Wi-Fi.

1. When an update is ready, tap the **Virtue update available** notification. You can also tap **Install Update** in the Virtue app.
2. The first time, Android says your phone isn't allowed to install unknown apps from this source. Tap **Settings**, then turn on **Allow from this source**. On Android 10 and 11, go back afterward.
3. Tap **Update**.

On Android 12 and later, Virtue installs later updates by itself. On Android 10 and 11, repeat the last step for each update.

## Usage

_Instructions coming soon._
