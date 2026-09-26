use super::PQC;
use blake3::Hasher;
use once_cell::sync::Lazy;
use std::sync::RwLock;

pub struct MockPQC;
const SK_LEN: usize = 32;
const SIG_LEN: usize = 32;
static REGISTRY: Lazy<RwLock<Vec<(Vec<u8>, Vec<u8>)>>> = Lazy::new(|| RwLock::new(Vec::new()));
impl PQC for MockPQC {
    const ALG: &'static str = "mock-blake3";
    fn keypair() -> (Vec<u8>, Vec<u8>) {
        thread_local! { static COUNTER: std::cell::RefCell<u64> = std::cell::RefCell::new(0); }
        let ctr = COUNTER.with(|c| {
            let mut v = c.borrow_mut();
            *v += 1;
            *v
        });
        let mut sk = [0u8; SK_LEN];
        let derived = blake3::derive_key("dyt-mock-sk", &ctr.to_le_bytes());
        sk.copy_from_slice(&derived);
        let pk = blake3::hash(&sk).as_bytes().to_vec();
        REGISTRY.write().unwrap().push((sk.to_vec(), pk.clone()));
        (sk.to_vec(), pk)
    }
    fn sign(sk: &[u8], msg: &[u8]) -> Vec<u8> {
        let mut h = Hasher::new();
        h.update(sk);
        h.update(msg);
        h.finalize().as_bytes()[..SIG_LEN].to_vec()
    }
    fn verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
        if let Some((sk, _)) = REGISTRY
            .read()
            .unwrap()
            .iter()
            .find(|(s, p)| blake3::hash(s).as_bytes() == pk && p == &pk)
        {
            let mut h = Hasher::new();
            h.update(sk);
            h.update(msg);
            let expected = h.finalize();
            return &expected.as_bytes()[..SIG_LEN] == sig;
        }
        false
    }
}
