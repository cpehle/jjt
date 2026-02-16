use rand::Rng;

pub fn generate() -> String {
    let n: u16 = rand::thread_rng().gen();
    format!("jt-{:04x}", n)
}
