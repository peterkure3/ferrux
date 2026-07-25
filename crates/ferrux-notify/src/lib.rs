use ferrux_core::ports::notification_sink::{Notification, NotificationSink};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::UI::WindowsAndMessaging::MB_OK;
use winrt_notification::Toast;

/// Fires a Windows toast plus a system beep. Not registered under a real
/// AppUserModelID, so toasts show up as coming from PowerShell — that's
/// the documented workaround (`Toast::POWERSHELL_APP_ID`) for apps that
/// aren't installed/registered.
pub struct WindowsNotifier;

impl NotificationSink for WindowsNotifier {
    fn notify(&self, notification: Notification) {
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(&notification.title)
            .text1(&notification.body)
            .show();
        unsafe {
            MessageBeep(MB_OK);
        }
    }
}
