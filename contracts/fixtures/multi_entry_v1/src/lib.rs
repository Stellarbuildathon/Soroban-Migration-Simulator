#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    User(Address),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserRecord {
    pub owner: Address,
    pub balance: u64,
}

#[contract]
pub struct MultiEntryV1Contract;

#[contractimpl]
impl MultiEntryV1Contract {
    pub fn set_user(env: Env, owner: Address, balance: u64) {
        owner.require_auth();
        let key = DataKey::User(owner.clone());
        let record = UserRecord {
            owner: owner.clone(),
            balance,
        };
        env.storage().persistent().set(&key, &record);
    }

    pub fn get_user(env: Env, owner: Address) -> Option<UserRecord> {
        let key = DataKey::User(owner);
        env.storage().persistent().get(&key)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    #[test]
    fn test_set_and_get_multiple_users() {
        let env = Env::default();
        let contract_id = env.register(MultiEntryV1Contract, ());
        let client = MultiEntryV1ContractClient::new(&env, &contract_id);

        let user_a = Address::generate(&env);
        let user_b = Address::generate(&env);
        let user_c = Address::generate(&env);
        env.mock_all_auths();

        client.set_user(&user_a, &100);
        client.set_user(&user_b, &250);
        client.set_user(&user_c, &500);

        let rec_a = client.get_user(&user_a).unwrap();
        let rec_b = client.get_user(&user_b).unwrap();
        let rec_c = client.get_user(&user_c).unwrap();

        assert_eq!(rec_a.balance, 100);
        assert_eq!(rec_b.balance, 250);
        assert_eq!(rec_c.balance, 500);
    }
}
