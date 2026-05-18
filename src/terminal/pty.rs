use std::sync::mpsc;

pub struct PtyHandle;

pub fn spawn_pty(_cols: u16, _rows: u16) -> (PtyHandle, mpsc::Receiver<Vec<u8>>) {
    let (_tx, rx) = mpsc::channel();
    (PtyHandle, rx)
}
