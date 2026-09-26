//! Test contract for the existing host ABI. This is not a mainnet registry.
#[cfg(target_arch = "wasm32")]
mod contract {
    use serde::{Deserialize, Serialize};

    const BUFFER_SIZE: usize = 16_384;
    #[link(wasm_import_module = "env")]
    extern "C" {
        fn read_input(ptr: *mut u8, max_len: i32) -> i32;
        fn write_output(ptr: *const u8, len: i32);
        fn get_caller_address(ptr: *mut u8, max_len: i32) -> i32;
        fn storage_get(key: *const u8, key_len: i32, value: *mut u8, max_len: i32) -> i32;
        fn storage_set(key: *const u8, key_len: i32, value: *const u8, len: i32) -> i32;
    }

    #[derive(Deserialize)]
    enum Method {
        RegisterAsset { hash: String, uri: String },
        GetAsset { id: u64 },
    }

    #[derive(Serialize, Deserialize)]
    struct Asset {
        id: u64,
        owner: String,
        hash: String,
        uri: String,
    }

    fn read(key: &[u8]) -> Option<Vec<u8>> {
        let mut data = vec![0; BUFFER_SIZE];
        let len = unsafe {
            storage_get(
                key.as_ptr(),
                key.len() as i32,
                data.as_mut_ptr(),
                BUFFER_SIZE as i32,
            )
        };
        if len < 0 {
            return None;
        }
        assert!((len as usize) < BUFFER_SIZE, "fixture value too large");
        data.truncate(len as usize);
        Some(data)
    }

    fn write(key: &[u8], value: &[u8]) {
        assert!(value.len() < BUFFER_SIZE, "fixture value too large");
        assert_eq!(
            unsafe {
                storage_set(
                    key.as_ptr(),
                    key.len() as i32,
                    value.as_ptr(),
                    value.len() as i32,
                )
            },
            0
        );
    }

    #[no_mangle]
    pub extern "C" fn handle() {
        let mut input = vec![0; BUFFER_SIZE];
        let len = unsafe { read_input(input.as_mut_ptr(), BUFFER_SIZE as i32) };
        assert!(
            len >= 0 && (len as usize) < BUFFER_SIZE,
            "fixture input too large"
        );
        let method: Method =
            serde_json::from_slice(&input[..len as usize]).expect("registry request");
        let output = match method {
            Method::RegisterAsset { hash, uri } => {
                let last: u64 = read(b"registry:last")
                    .map(|v| serde_json::from_slice(&v).unwrap())
                    .unwrap_or(0);
                let id = last.checked_add(1).expect("asset id overflow");
                let mut caller = vec![0; 256];
                let len = unsafe { get_caller_address(caller.as_mut_ptr(), caller.len() as i32) };
                assert!(len >= 0 && (len as usize) < caller.len());
                caller.truncate(len as usize);
                let asset = Asset {
                    id,
                    owner: String::from_utf8(caller).unwrap(),
                    hash,
                    uri,
                };
                write(
                    format!("registry:asset:{id}").as_bytes(),
                    &serde_json::to_vec(&asset).unwrap(),
                );
                let encoded = serde_json::to_vec(&id).unwrap();
                write(b"registry:last", &encoded);
                encoded
            }
            Method::GetAsset { id } => {
                read(format!("registry:asset:{id}").as_bytes()).unwrap_or_else(|| b"null".to_vec())
            }
        };
        unsafe { write_output(output.as_ptr(), output.len() as i32) };
    }
}
