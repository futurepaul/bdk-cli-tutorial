use bdk::{descriptor::Descriptor, miniscript::DescriptorPublicKey, wallet::AddressInfo};
use std::str::FromStr;

use anyhow::{bail, ensure, Context, Result};
use bdk::bitcoin::{
    self,
    consensus::{deserialize, encode::serialize},
    util::psbt::PartiallySignedTransaction,
    Address,
};
use bdk::blockchain::{noop_progress, ElectrumBlockchain};
use bdk::database::MemoryDatabase;
use bdk::electrum_client::Client;
use bdk::wallet::AddressIndex;
use bdk::Wallet;

#[derive(Debug, Clone)]
enum Mode {
    Balance {
        descriptor: String,
    },
    Receive {
        descriptor: String,
        index: u32,
    },
    Send {
        descriptor: String,
        change_descriptor: String,
        amount: u64,
        destination: String,
    },
    Broadcast {
        descriptor: String,
        psbt: String,
    },
}

fn main() {
    let mode = match parse_args() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {}.", e);
            std::process::exit(1);
        }
    };

    match execute(mode) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {}.", e);
            std::process::exit(1);
        }
    }
}

fn execute(mode: Mode) -> Result<()> {
    match mode {
        Mode::Balance { descriptor } => {
            // No need for a change address because we're just checking the balance
            let wallet = create_wallet(&descriptor, None)?;

            // Get the balance in sats
            // It's a sum of the unspent outputs known to the wallet's internal database (so you need to sync first)
            let balance = wallet.get_balance()?;
            println!("{} sats", balance);

            // List unspent ouputs
            println!("{:#?}", wallet.list_unspent());

            Ok(())
        }
        Mode::Receive { descriptor, index } => {
            let wallet = create_wallet(&descriptor, None)?;

            // Derives an address based on the wallet's descriptor and the given index
            let info = wallet.get_address(AddressIndex::Peek(index))?;

            // AddressInfo automatically derefs to and displays as an address, but it also includes the index if we need it
            let AddressInfo { index, address } = info;

            // Create a descriptor manually from the descriptor string
            let underived_desc: Descriptor<DescriptorPublicKey> = bdk::miniscript::Descriptor::from_str(&descriptor)?;

            println!("underived descriptor: {}", underived_desc);

            // Now we can derive a descriptor of the specific index.
            // We can use this with hwi's `displayaddress` method
            let desc: Descriptor<DescriptorPublicKey> = underived_desc.derive(index);

            // We could use rust-hwi to verify this address from within our "app"
            // But let's just do it manually for now
            // hwi -t "coldcard" displayaddress --desc "..."
            println!("descriptor: {}", desc);
            println!("index: {}", index);
            println!("address: {}", address);

            Ok(())
        }
        Mode::Send {
            descriptor,
            change_descriptor,
            amount,
            destination,
        } => {
            let wallet = create_wallet(&descriptor, Some(&change_descriptor))?;

            // Use rust-bitcoin to parse the address string into its `Address` type
            // Then convert this address into a script pubkey that spends to it
            let dest_script = Address::from_str(destination.as_str())?.script_pubkey();

            // Create a blank `TxBuilder`
            let mut tx_builder = wallet.build_tx();

            // Add our script and the amount in sats to send
            tx_builder.add_recipient(dest_script, amount);

            // "Finish" the builder which returns a tuple:
            // A `PartiallySignedTransaction` which serializes as a psbt
            // And `TransactionDetails` which has helpful info about the transaction we just built
            let (psbt, details) = tx_builder.finish()?;
            println!("{:#?}", details);
            println!("{}", base64::encode(&serialize(&psbt)));

            Ok(())
        }
        Mode::Broadcast { descriptor, psbt } => {
            let wallet = create_wallet(&descriptor, None)?;

            // Deserialize the psbt. First as a Vec of bytes, then as a strongly typed `PartiallySignedTransaction`
            let psbt = base64::decode(&psbt)?;
            let psbt: PartiallySignedTransaction = deserialize(&psbt)?;

            // TKTK
            let tx = psbt.extract_tx();

            // Broadcast the transaction using our chosen backend, returning a `Txid` or an error
            let txid = wallet.broadcast(tx)?;
            
            println!("{:#?}", txid);

            Ok(())
        }
    }
}

fn create_wallet(desc_string: &str, change_desc: Option<&str>) -> Result<Wallet<ElectrumBlockchain, MemoryDatabase>> {
    // Create a SSL-encrypted Electrum client
    let client = Client::new("ssl://electrum.blockstream.info:60002")?;

    // Create a BDK wallet
    let wallet = Wallet::new(
        // Our wallet descriptor
        desc_string,
        // Descriptor used for generating change addresses
        change_desc,
        // Which network we'll using. If you change this to `Bitcoin` things get real.
        bitcoin::Network::Testnet,
        // In-memory ephemeral database. There's also a default key value storage provided by BDK if you want persistence.
        MemoryDatabase::default(),
        // This wrapper implements the blockchain traits BDK needs for this specific client type
        ElectrumBlockchain::from(client),
    )?;

    println!("Syncing...");

    // Important! We have to sync our wallet with the blockchain.
    // Because our wallet is ephemeral we need to do this on each run, so I put it in `create_wallet` for convenience.
    wallet.sync(noop_progress(), None)?;

    Ok(wallet)
}

fn parse_args() -> Result<Mode> {
    let mut pargs = pico_args::Arguments::from_env();
    let subcommand = pargs.subcommand()?;

    ensure!(
        subcommand.is_some(),
        "Need to pick a mode: balance || receive || send || broadcast"
    );

    let descriptor: String = pargs
        .free_from_str()
        .context("Need to include a descriptor")?;

    let info = match subcommand.unwrap().as_str() {
        "balance" => Mode::Balance { descriptor },
        "receive" => Mode::Receive {
            descriptor,
            index: pargs
                .value_from_str("--index")
                .context("Missing index argument")?,
        },
        "send" => Mode::Send {
            descriptor,
            change_descriptor: pargs.value_from_str("--change").context("Missing change descriptor")?,
            amount: pargs.value_from_str("--amount").context("Missing amount")?,
            destination: pargs
                .value_from_str("--dest")
                .context("Missing destination address")?,
        },
        "broadcast" => Mode::Broadcast {
            descriptor,
            psbt: pargs.value_from_str("--psbt").context("Missing PSBT")?,
        },
        _ => bail!("Unknown mode"),
    };

    Ok(info)
}
