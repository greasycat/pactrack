use log::debug;

const SUMMARY: &str = "Pactrack";
const ICON: &str = "software-update-available";

pub fn notify_count_change(previous: usize, current: usize) {
    let body = notification_body(previous, current);

    let result = notify_rust::Notification::new()
        .summary(SUMMARY)
        .body(&body)
        .icon(ICON)
        .show();

    if let Err(err) = result {
        debug!("failed to send desktop notification: {err}");
    }
}

fn notification_body(previous: usize, current: usize) -> String {
    format!("Pending updates changed from {} to {}", previous, current)
}

#[cfg(test)]
mod tests {
    use super::{notification_body, notify_count_change};

    #[test]
    fn notification_body_formats_counts() {
        let body = notification_body(2, 5);
        assert_eq!(body, "Pending updates changed from 2 to 5");
    }

    #[test]
    fn notification_body_handles_zero_counts() {
        let body = notification_body(0, 0);
        assert_eq!(body, "Pending updates changed from 0 to 0");
    }

    #[test]
    fn notification_test_sends_notification() {
        let previous = 2;
        let current = 5;
        notify_count_change(previous, current);
    }
}
