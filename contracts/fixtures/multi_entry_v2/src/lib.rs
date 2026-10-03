#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

#[contracttype]
pub enum DataKey {
    User(Address),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserRecordV1 {
    pub owner: Address,
    pub balance: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserRecordV2 {
    pub owner: Address,
    pub balance: u64,
    pub version: u32,
}

#[contract]
pub struct MultiEntryV2Contract;

#[contractimpl]
impl MultiEntryV2Contract {
    pub fn set_user(env: Env, owner: Address, balance: u64) {
        owner.require_auth();
        let key = DataKey::User(owner.clone());
        let record = UserRecordV2 {
            owner: owner.clone(),
            balance,
            version: 2,
        };
        env.storage().persistent().set(&key, &record);
    }

    pub fn get_user(env: Env, owner: Address) -> Option<UserRecordV2> {
        let key = DataKey::User(owner);
        env.storage().persistent().get(&key)
    }

    /// Migrate a single user record from V1 to V2 schema.
    pub fn migrate_user(env: Env, owner: Address) {
        let key = DataKey::User(owner.clone());
        if let Some(old) = env.storage().persistent().get::<_, UserRecordV1>(&key) {
            let updated = UserRecordV2 {
                owner: old.owner,
                balance: old.balance,
                version: 2,
            };
            env.storage().persistent().set(&key, &updated);
        } else {
            panic!("user record not found");
        }
    }

    /// Batch migrate multiple user records from V1 to V2 schema.
    pub fn migrate_users(env: Env, users: Vec<Address>) {
        for user in users.iter() {
            Self::migrate_user(env.clone(), user);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, vec, Env};

    #[test]
    fn test_migrate_single_user() {
        let env = Env::default();
        let contract_id = env.register(MultiEntryV2Contract, ());
        let client = MultiEntryV2ContractClient::new(&env, &contract_id);

        let user = Address::generate(&env);

        // Seed V1 state
        let key = DataKey::User(user.clone());
        let old_rec = UserRecordV1 {
            owner: user.clone(),
            balance: 777,
        };
        env.as_contract(&contract_id, || {
            env.storage().persistent().set(&key, &old_rec);
        });

        // Migrate
        client.migrate_user(&user);

        let new_rec = client.get_user(&user).unwrap();
        assert_eq!(new_rec.owner, user);
        assert_eq!(new_rec.balance, 777);
        assert_eq!(new_rec.version, 2);
    }

    #[test]
    fn test_batch_migrate_users() {
        let env = Env::default();
        let contract_id = env.register(MultiEntryV2Contract, ());
        let client = MultiEntryV2ContractClient::new(&env, &contract_id);

        let user_a = Address::generate(&env);
        let user_b = Address::generate(&env);
        let user_c = Address::generate(&env);

        // Seed V1 state for all 3 users
        env.as_contract(&contract_id, || {
            env.storage().persistent().set(
                &DataKey::User(user_a.clone()),
                &UserRecordV1 {
                    owner: user_a.clone(),
                    balance: 100,
                },
            );
            env.storage().persistent().set(
                &DataKey::User(user_b.clone()),
                &UserRecordV1 {
                    owner: user_b.clone(),
                    balance: 200,
                },
            );
            env.storage().persistent().set(
                &DataKey::User(user_c.clone()),
                &UserRecordV1 {
                    owner: user_c.clone(),
                    balance: 300,
                },
            );
        });

        let users_to_migrate = vec![&env, user_a.clone(), user_b.clone(), user_c.clone()];
        client.migrate_users(&users_to_migrate);

        let rec_a = client.get_user(&user_a).unwrap();
        let rec_b = client.get_user(&user_b).unwrap();
        let rec_c = client.get_user(&user_c).unwrap();

        assert_eq!(rec_a.version, 2);
        assert_eq!(rec_a.balance, 100);
        assert_eq!(rec_b.version, 2);
        assert_eq!(rec_b.balance, 200);
        assert_eq!(rec_c.version, 2);
        assert_eq!(rec_c.balance, 300);
    }
}
