use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct CounterState {
    pub count: u64,
}

#[no_mangle]
pub extern "C" fn init() -> *const u8 {
    let state = CounterState { count: 0 };
    let json = serde_json::to_string(&state).unwrap();
    let ptr = json.as_ptr();
    std::mem::forget(json);
    ptr
}

#[no_mangle]
pub extern "C" fn increment() -> *const u8 {
    // In real implementation, load state from storage
    let mut state = CounterState { count: 1 }; // Simplified
    state.count += 1;
    let json = serde_json::to_string(&state).unwrap();
    let ptr = json.as_ptr();
    std::mem::forget(json);
    ptr
}

#[no_mangle]
pub extern "C" fn get() -> *const u8 {
    let state = CounterState { count: 2 }; // Simplified
    let json = serde_json::to_string(&state).unwrap();
    let ptr = json.as_ptr();
    std::mem::forget(json);
    ptr
}
