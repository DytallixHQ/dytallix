#[no_mangle]
pub extern "C" fn ping() -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::ping;

    #[test]
    fn ping_returns_success_code() {
        assert_eq!(ping(), 1);
    }
}
