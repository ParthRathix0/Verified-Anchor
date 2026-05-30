use verified_anchor::account;

#[account(some_flag)]
pub struct BadAttr {
    pub field: u64,
}

fn main() {}
