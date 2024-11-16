#[macro_export]
/// Shorthand for sending AppUpdates with a synchronous sender
macro_rules! broadcast {
    ($tx:expr, $data:expr) => {
        $tx.send($data.into()).expect("Failed to broadcast message");
    };
    ($tx:expr, $data:expr, $err_msg:expr) => {
        $tx.send($data.into()).expect($err_msg);
    };
}
