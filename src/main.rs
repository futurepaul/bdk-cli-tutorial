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

    // println!("{:#?}", mode.clone());

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
            let wallet = create_wallet(descriptor, None)?;

            let balance = wallet.get_balance()?;
            println!("{} sats", balance);

            println!("{:#?}", wallet.list_unspent());

            Ok(())
        }
        Mode::Receive { descriptor, index } => {
            let wallet = create_wallet(descriptor.clone(), None)?;

            let info = wallet.get_address(AddressIndex::Peek(index))?;

            let AddressInfo { index, address } = info;

            let underived_desc: Descriptor<DescriptorPublicKey> = bdk::miniscript::Descriptor::from_str(&descriptor)?;

            println!("underived descriptor: {}", underived_desc);

            let desc: Descriptor<DescriptorPublicKey> = underived_desc.derive(index);

            // Could use rust-hwi to verify this address
            // Or just hwi -t "coldcard" displayaddress --desc "..."
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
            let wallet = create_wallet(descriptor, Some(&change_descriptor))?;

            let dest_script = Address::from_str(destination.as_str())
                .unwrap()
                .script_pubkey();

            let mut tx_builder = wallet.build_tx();

            tx_builder.add_recipient(dest_script, amount);
            let (psbt, details) = tx_builder.finish()?;
            println!("{:#?}", details);
            println!("{}", base64::encode(&serialize(&psbt)));

            Ok(())
        }
        Mode::Broadcast { descriptor, psbt } => {
            let wallet = create_wallet(descriptor, None)?;

            let psbt = base64::decode(&psbt)?;
            let psbt: PartiallySignedTransaction = deserialize(&psbt)?;

            let txid = wallet.broadcast(psbt.extract_tx())?;
            println!("{:#?}", txid);

            Ok(())
        }
    }
}

fn create_wallet(desc_string: String, change_desc: Option<&str>) -> Result<Wallet<ElectrumBlockchain, MemoryDatabase>> {
    let client = Client::new("ssl://electrum.blockstream.info:60002")?;
    let wallet = Wallet::new(
        desc_string.as_str(),
        change_desc,
        bitcoin::Network::Testnet,
        MemoryDatabase::default(),
        ElectrumBlockchain::from(client),
    )?;

    println!("Syncing...");
    wallet.sync(noop_progress(), None)?;

    Ok(wallet)
}

fn parse_args() -> Result<Mode> {
    let mut pargs = pico_args::Arguments::from_env();
    let subcommand = pargs.subcommand()?;

    ensure!(
        subcommand.is_some(),
        "Need to pick a mode: balance || receive || send"
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
