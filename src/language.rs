use std::sync::atomic::{AtomicBool, Ordering};

static CHINESE: AtomicBool = AtomicBool::new(false);

pub fn text<'a>(english: &'a str, chinese: &'a str) -> &'a str {
    if CHINESE.load(Ordering::Relaxed) {
        chinese
    } else {
        english
    }
}

pub fn set_chinese(chinese: bool) {
    CHINESE.store(chinese, Ordering::Relaxed);
}
