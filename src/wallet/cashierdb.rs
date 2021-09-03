use crate::client::ClientFailed;
use crate::serial;
use crate::serial::{deserialize, serialize, Decodable, Encodable};
use crate::service::btc::{PrivKey, PubKey};
use crate::util::join_config_path;
use crate::{Error, Result};

use async_std::sync::Arc;
use ff::Field;
use log::*;
use rand::rngs::OsRng;
use rusqlite::{named_params, params, Connection};

use std::path::PathBuf;

pub type CashierDbPtr = Arc<CashierDb>;

pub struct CashierDb {
    pub path: PathBuf,
    pub cashier_secrets: Vec<jubjub::Fr>,
    pub cashier_public: jubjub::SubgroupPoint,
    pub password: String,
}

impl CashierDb {
    pub fn new(wallet: &str, password: String) -> Result<Self> {
        debug!(target: "CASHIERDB", "new() Constructor called");
        let path = join_config_path(&PathBuf::from(wallet))?;
        let cashier_secret = jubjub::Fr::random(&mut OsRng);
        let cashier_public = zcash_primitives::constants::SPENDING_KEY_GENERATOR * cashier_secret;
        Ok(Self {
            path,
            cashier_secrets: vec![cashier_secret.clone()],
            cashier_public,
            password,
        })
    }

    pub fn init_db(&self) -> Result<()> {
        if !self.password.trim().is_empty() {
            let contents = include_str!("../../res/cashier.sql");
            let conn = Connection::open(&self.path)?;
            debug!(target: "CASHIERDB", "Opened connection at path {:?}", self.path);
            conn.pragma_update(None, "key", &self.password)?;
            conn.execute_batch(&contents)?;
        } else {
            debug!(target: "CASHIERDB", "Password is empty. You must set a password to use the wallet.");
            return Err(Error::from(ClientFailed::EmptyPassword));
        }
        Ok(())
    }

    pub fn get_keys_by_dkey(&self, dkey_pub: &Vec<u8>) -> Result<()> {
        debug!(target: "CASHIERDB", "Check for existing dkey");
        //let dkey_id = self.get_value_deserialized(dkey_pub)?;
        // open connection
        let conn = Connection::open(&self.path)?;
        // unlock database
        conn.pragma_update(None, "key", &self.password)?;

        // let mut keypairs = conn.prepare("SELECT dkey_id FROM keypairs WHERE dkey_id = :dkey_id")?;
        // let rows = keypairs.query_map::<Vec<u8>, _, _>(&[(":dkey_id", &secret)], |row| row.get(0))?;

        let mut stmt = conn.prepare("SELECT * FROM keypairs where dkey_id = ?")?;
        let mut rows = stmt.query([dkey_pub])?;
        if let Some(_row) = rows.next()? {
            println!("Got something");
        } else {
            println!("Did not get something");
        }

        Ok(())
    }

    // Update to take BitcoinKeys instance instead
    pub fn put_exchange_keys(
        &self,
        dkey_pub: Vec<u8>,
        btc_private: PrivKey,
        btc_public: PubKey,
        //txid will be updated when exists
    ) -> Result<()> {
        debug!(target: "CASHIERDB", "Put exchange keys");
        // prepare the values
        //let dkey_pub = self.get_value_serialized(&dkey_pub)?;
        let btc_private = btc_private.to_bytes();
        let btc_public = btc_public.to_bytes();

        // open connection
        let conn = Connection::open(&self.path)?;
        // unlock database
        conn.pragma_update(None, "key", &self.password)?;

        conn.execute(
            "INSERT INTO keypairs(dkey_id, btc_key_private, btc_key_public)
            VALUES (:dkey_id, :btc_key_private, :btc_key_public)",
            named_params! {
                ":dkey_id": dkey_pub,
                ":btc_key_private": btc_private,
                ":btc_key_private": btc_public,
            },
        )?;
        Ok(())
    }

    // return (private key, public key)
    pub fn get_address_by_btc_key(
        &self,
        btc_address: &Vec<u8>,
    ) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        debug!(target: "CASHIERDB", "Check for existing btc address");
        // open connection
        let conn = Connection::open(&self.path)?;
        // unlock database
        conn.pragma_update(None, "key", &self.password)?;

        let mut stmt =
            conn.prepare("SELECT * FROM withdraw_keypairs where btc_key_id = :btc_key_id")?;
        let addr_iter = stmt
            .query_map::<(Vec<u8>, Vec<u8>), _, _>(&[(":btc_key_id", btc_address)], |row| {
                Ok((row.get(1)?, row.get(2)?))
            })?;

        let mut btc_addresses = vec![];

        for addr in addr_iter {
            btc_addresses.push(addr);
        }

        if let Some(addr) = btc_addresses.pop() {
            return Ok(Some(addr?));
        }

        return Ok(None);
    }

    pub fn put_withdraw_keys(
        &self,
        btc_key_id: Vec<u8>,
        d_key_private: Vec<u8>,
        d_key_public: Vec<u8>,
    ) -> Result<()> {
        debug!(target: "CASHIERDB", "Put withdraw keys");

        // open connection
        let conn = Connection::open(&self.path)?;
        // unlock database
        conn.pragma_update(None, "key", &self.password)?;

        conn.execute(
            "INSERT INTO withdraw_keypairs(btc_key_id, d_key_private, d_key_public) 
            VALUES (:btc_key_id, :d_key_private, :d_key_public)",
            named_params! {
                ":btc_key_id": btc_key_id,
                ":d_key_private": d_key_private,
                ":d_key_public": d_key_public,
            },
        )?;
        Ok(())
    }

    pub fn cash_key_gen(&self) -> (Vec<u8>, Vec<u8>) {
        debug!(target: "CASHIERDB", "Generating cashier keys...");
        let secret: jubjub::Fr = jubjub::Fr::random(&mut OsRng);
        let public = zcash_primitives::constants::SPENDING_KEY_GENERATOR * secret;
        let pubkey = serial::serialize(&public);
        let privkey = serial::serialize(&secret);
        (pubkey, privkey)
    }

    pub fn put_keypair(&self, key_public: Vec<u8>, key_private: Vec<u8>) -> Result<()> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "key", &self.password)?;
        conn.execute(
            "INSERT INTO keys(key_public, key_private) VALUES (?1, ?2)",
            params![key_public, key_private],
        )?;
        Ok(())
    }

    pub fn put_cashier_pub(&self, key_public: Vec<u8>) -> Result<()> {
        debug!(target: "CASHIERDB", "Save cashier keys...");
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "key", &self.password)?;
        conn.execute(
            "INSERT INTO cashier(key_public) VALUES (?1)",
            params![key_public],
        )?;
        Ok(())
    }

    pub fn get_cashier_public(&self) -> Result<jubjub::SubgroupPoint> {
        debug!(target: "CASHIERDB", "Returning keys...");
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "key", &self.password)?;
        let mut stmt = conn.prepare("SELECT key_public FROM keys")?;
        let key_iter = stmt.query_map::<Vec<u8>, _, _>([], |row| row.get(0))?;
        let mut pub_keys = Vec::new();
        for key in key_iter {
            pub_keys.push(key?);
        }
        let public: jubjub::SubgroupPoint = self.get_value_deserialized(
            pub_keys
                .pop()
                .expect("unable to load public_key from cashierdb"),
        )?;
        Ok(public)
    }
    pub fn get_cashier_private(&self) -> Result<jubjub::Fr> {
        debug!(target: "CASHIERDB", "Returning keys...");
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "key", &self.password)?;
        let mut stmt = conn.prepare("SELECT key_private FROM keys")?;
        let key_iter = stmt.query_map::<Vec<u8>, _, _>([], |row| row.get(0))?;
        let mut keys = Vec::new();
        for key in key_iter {
            keys.push(key?);
        }
        let private: jubjub::Fr = self.get_value_deserialized(
            keys.pop()
                .expect("unable to load private_key from cashierdb"),
        )?;
        Ok(private)
    }

    pub fn test_wallet(&self) -> Result<()> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "key", &self.password)?;
        let mut stmt = conn.prepare("SELECT * FROM keys")?;
        let _rows = stmt.query([])?;
        Ok(())
    }

    pub fn get_value_serialized<T: Encodable>(&self, data: &T) -> Result<Vec<u8>> {
        let v = serialize(data);
        Ok(v)
    }

    pub fn get_value_deserialized<D: Decodable>(&self, key: Vec<u8>) -> Result<D> {
        let v: D = deserialize(&key)?;
        Ok(v)
    }
}
