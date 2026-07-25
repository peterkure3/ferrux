pub struct Notification {
    pub title: String,
    pub body: String,
}

pub trait NotificationSink {
    fn notify(&self, notification: Notification);
}
