use super::{Migration, SimpleSqlMigration};

pub fn migration() -> impl Migration {
    SimpleSqlMigration {
        serial_number: 0,
        sql: vec![
            // Addresses can be encoded as either bech32 or base58check
            r#"
            CREATE TABLE users (
                id UUID PRIMARY KEY,
                email TEXT UNIQUE NOT NULL,
                password TEXT NOT NULL,
                balance_msats BIGINT NOT NULL,
                created TIMESTAMP WITH TIME ZONE NOT NULL
            )"#,
            r#"CREATE INDEX user_email ON users (email)"#,
            r#"
            CREATE TABLE auth_tokens (
                id UUID PRIMARY KEY,
                user_id UUID NOT NULL REFERENCES users,
                name TEXT UNIQUE NOT NULL,
                token_hash TEXT UNIQUE NOT NULL,
                can_spend BOOLEAN NOT NULL,
                can_receive BOOLEAN NOT NULL,
                can_read BOOLEAN NOT NULL,
                created TIMESTAMP WITH TIME ZONE NOT NULL,
                disabled TIMESTAMP WITH TIME ZONE
            )"#,
            r#"
            CREATE TABLE balance_reservations (
                id UUID PRIMARY KEY,
                user_id UUID NOT NULL REFERENCES users,
                amount_msats BIGINT NOT NULL,
                status INT NOT NULL,
                created TIMESTAMP WITH TIME ZONE
            )"#,
            r#"
            CREATE TABLE tx_outs (
                tx_id TEXT NOT NULL,
                block_height INT,
                address TEXT NOT NULL,
                v_out INT NOT NULL,
                amount_sats BIGINT NOT NULL,
                PRIMARY KEY (tx_id, v_out)
            )
            "#,
            r#"CREATE INDEX tx_out_block_height ON tx_outs (block_height)"#,
            r#"
            CREATE TABLE deposit_addresses (
                user_id UUID NOT NULL REFERENCES users,
                token_id UUID NOT NULL REFERENCES auth_tokens,
                address TEXT NOT NULL PRIMARY KEY,
                created TIMESTAMP WITH TIME ZONE NOT NULL
            )
            "#,
            r#"
            CREATE TABLE deposits (
                id UUID PRIMARY KEY,
                user_id UUID NOT NULL REFERENCES users,
                tx_id TEXT NOT NULL,
                v_out INT NOT NULL,
                address TEXT NOT NULL REFERENCES deposit_addresses,
                created TIMESTAMP WITH TIME ZONE NOT NULL,
                confirmed TIMESTAMP WITH TIME ZONE,
                FOREIGN KEY (tx_id, v_out) REFERENCES tx_outs (tx_id, v_out)
            )
            "#,
            r#"CREATE UNIQUE INDEX deposit_tx_id_v_out ON deposits (tx_id, v_out)"#,
            r#"
            CREATE TABLE withdrawals (
                id UUID PRIMARY KEY,
                user_id UUID NOT NULL REFERENCES users,
                token_id UUID NOT NULL REFERENCES auth_tokens,
                reservation_id UUID NOT NULL REFERENCES balance_reservations,
                address TEXT NOT NULL,
                fee_sats BIGINT NOT NULL,
                amount_sats BIGINT NOT NULL,
                tx_id TEXT,
                v_out INT,
                created TIMESTAMP WITH TIME ZONE NOT NULL,
                confirmed TIMESTAMP WITH TIME ZONE,
                FOREIGN KEY (tx_id, v_out) REFERENCES tx_outs (tx_id, v_out)
            )
            "#,
            r#"CREATE UNIQUE INDEX withdrawal_tx_id_v_out ON withdrawals (tx_id, v_out)"#,
            r#"
            CREATE TABLE payments (
                id UUID PRIMARY KEY,
                user_id UUID NOT NULL REFERENCES users,
                token_id UUID NOT NULL REFERENCES auth_tokens,
                reservation_id UUID REFERENCES balance_reservations,
                amount_msats BIGINT NOT NULL,
                fee_msats BIGINT,
                invoice TEXT NOT NULL,
                created TIMESTAMP WITH TIME ZONE NOT NULL,
                status INT NOT NULL,
                failure_reason TEXT,
                failure_timestamp TIMESTAMP WITH TIME ZONE,
                success_timestamp TIMESTAMP WITH TIME ZONE
            )
            "#,
            r#"
            CREATE TABLE invoices (
                id UUID PRIMARY KEY,
                user_id UUID NOT NULL REFERENCES users,
                token_id UUID NOT NULL REFERENCES auth_tokens,
                amount_msats BIGINT NOT NULL,
                memo TEXT,
                invoice TEXT NOT NULL UNIQUE,
                created TIMESTAMP WITH TIME ZONE NOT NULL,
                expiration TIMESTAMP WITH TIME ZONE,
                settlement_amount BIGINT,
                settlement_timestamp TIMESTAMP WITH TIME ZONE,
                settle_index BIGINT
            )
            "#,
            r#"CREATE INDEX invoice_index ON invoices (invoice)"#,
            r#"CREATE INDEX invoice_settle_index ON invoices (settle_index)"#,
        ],
    }
}
