pub use hashbrown::HashMap;
pub use primitive_types::{H160, H256, U256};
pub use revm::{
    db::DatabaseRef, db::EmptyDB, Account, AccountInfo, Bytecode, Database, DatabaseCommit, Log,
    Return, TransactOut, TxEnv, EVM,
};

pub type State = HashMap<H160, Account>;

// mod db;

pub struct Rethnet<D: Database + DatabaseCommit> {
    evm: EVM<D>,
}

impl<D: Database + DatabaseCommit> Rethnet<D> {
    pub fn with_database(db: D) -> Self {
        let mut evm = EVM::new();
        evm.database(db);

        Self { evm }
    }

    // ?
    // TransactTo::Call & TransactTo::Create
    // For both cases, can we do a dry run and state-changing run?
    pub fn dry_run(&mut self, tx: TxEnv) -> (Return, TransactOut, u64, State, Vec<Log>) {
        self.evm.env.tx = tx;
        self.evm.transact()
    }

    pub fn run(&mut self, tx: TxEnv) -> (Return, TransactOut, u64, Vec<Log>) {
        self.evm.env.tx = tx;
        self.evm.transact_commit()
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn it_works() {}
// }