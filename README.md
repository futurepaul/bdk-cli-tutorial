# Building a CLI Bitcoin wallet with BDK and Rust

Today we're going to build a Bitcoin wallet. More specifically, a descriptor-based, watch-only wallet that can create PSBTs to be signed offline by a Coldcard, and broadcast those PSBTs once finalized.

If that sounds like a lot of novel concepts, you've come to the right place, because explaining and understanding these concepts is the whole point of this guide.

# Tools you'll need to follow this guide

* Rust installed (https://rustup.rs/)
* HWI installed (https://github.com/bitcoin-core/HWI, optional but very helpful)
* Coldcard or other wallet with descriptor support

# Before I waste your time

If you're already pretty comfortable with Bitcoin concepts and Rust, I highly recommend checking out BDK's own [`bdk-cli` example repo](https://github.com/bitcoindevkit/bdk-cli) for a much more in-depth implementation of what we're trying to accomplish here.

## What is Rust?

Rust is a low-level systems programming language with a lot of high-level ergonomics and, most importantly for our use, an extreme focus on "safety."

Here's the "reliability" blurb from the Rust lang website:

> Rust’s rich type system and ownership model guarantee memory-safety and thread-safety — enabling you to eliminate many classes of bugs at compile-time.

Since working with Bitcoin means working with money, we want to have as many assurances as possible that our software works as intended, and Rust's compiler adds a lot of peace of mind -- though obviously it's not a silver bullet.

## What is BDK

BDK is a Rust library for building Bitcoin wallets.

Here's what the GitHub says:

> A modern, lightweight, descriptor-based wallet library written in Rust!

The first thing to know is that BDK is built on a really high quality foundation: the [rust-bitcoin](https://github.com/rust-bitcoin/rust-bitcoin) library, and the [rust-miniscript](https://github.com/rust-bitcoin/rust-miniscript) library.

BDK provides the rest of the functionality you need to build a wallet out of the primitives provided by that foundation. This includes:

1. Interfacing with blockchain backends like Electrum (and soon Bitcoin Core RPC)
2. Storing the wallet state in a database 
3. Key management
4. Language bindings for mobile development (WIP, but C, Java/Kotlin, and Swift are planned)

In addition to this machinery, there's also a big emphasis on providing sane defaults in BDK, which is ideal for a developer like myself who lives in terror of losing someone's bitcoin. BDK makes it a little easier to match best practices for modern bitcoin usage and steer clear of sharp edges.

## Let's start coding though

## Step 1a: Project setup
First create a new binary rust project and add these dependencies to Cargo.toml:

```toml
anyhow = "1.0.40"
base64 = "0.13.0"
bdk = { git = "https://github.com/bitcoindevkit/bdk", rev="0ec064e" }
pico-args = "0.4.0"
```

(TODO: update bdk dep to 0.8 when it's released)

If you're absolutely new to Rust and what I said doesn't make any sense, the [official Rust Programming Language book](https://doc.rust-lang.org/stable/book/ch01-00-getting-started.html) is highly recommended.

## Setup 1b: Get your descriptors

If you installed HWI you should be able to run `hwi enumerate` and see a list of connected hardware wallets. If you see a wallet you can reference it by type and ask for a descriptor. In this case I'm asking for a Testnet descriptor.

```bash
hwi -t "coldcard" --chain test getdescriptors
```

That should output an array of "receive" descriptors and an array of "internal" descriptors. Grab the one from each list that starts with `wpkh`.

If you don't want to bother with getting HWI setup you can also export your Coldcard's descriptors to a .txt file and transfer it via microSD card:

`Advanced > MicroSD Card > Export Wallet > Bitcoin Core`

And if you don't have a spare hardware wallet you can run a [Coldcard simulator](https://github.com/Coldcard/firmware), or you can grab the sample descriptors from [this GitHub issue](https://github.com/Coldcard/firmware/pull/32) (though you won't be able to sign them for obvious reasons).

Once, by hook or by crook, you have your descriptors, I recommend saving them to a local `.env` file for easy reference from the cli we're building:

```bash
DESC="wpkh([0f056943/84h/1h/0h]tpubDC7jGaaSE66Pn4dgtbAAstde4bCyhSUs4r3P8WhMVvPByvcRrzrwqSvpF9Ghx83Z1LfVugGRrSBko5UEKELCz9HoMv5qKmGq3fqnnbS5E9r/0/*)#erexmnep"
CHANGE="wpkh([0f056943/84h/1h/0h]tpubDC7jGaaSE66Pn4dgtbAAstde4bCyhSUs4r3P8WhMVvPByvcRrzrwqSvpF9Ghx83Z1LfVugGRrSBko5UEKELCz9HoMv5qKmGq3fqnnbS5E9r/1/*)#ghu8xxfe"
```

Now just `source .env` and you can refer to the descriptors as `$DESC` and `$CHANGE`.

## Step 2: Argument parsing

To kick off our actual code for this project we'll want to parse some command line arguments. Because this isn't the main focus I'm going to speed through this. 

I'm using [pico-args](https://github.com/RazrFalcon/pico-args) for argument parsing and [anyhow](https://github.com/dtolnay/anyhow) for easy error handling.

```rust
use anyhow::{bail, ensure, Context, Result};

#[derive(Debug, Clone)]
enum Mode {
    Balance {
        descriptor: String,
        change_descriptor: String,
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

    println!("{:#?}", mode);
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
        "balance" => Mode::Balance {
            descriptor,
            change_descriptor: pargs
                .value_from_str("--change")
                .context("Missing change descriptor")?,
        },
        "receive" => Mode::Receive {
            descriptor,
            index: pargs
                .value_from_str("--index")
                .context("Missing index argument")?,
        },
        "send" => Mode::Send {
            descriptor,
            change_descriptor: pargs
                .value_from_str("--change")
                .context("Missing change descriptor")?,
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
```

Now we can kick the tires of our various subcommands.

```bash
cargo run -- balance $DESC --change $CHANGE
cargo run -- receive $DESC --index 123
cargo run -- send $DESC --change $CHANGE --amount 12345 --dest $RECV
cargo run -- broadcast $DESC --psbt abcdefg
```

Everything after the first `--` are arguments we're passing to our program, before the `--` are arguments we're passing to Rust's Cargo build tool.

If you'd like the "real cli experience" you can install your app like this (this will do a release build):

```
cargo install --path .
```

And then run commands like this:

```
./bdk-cli-tutorial balance $DESC --change $CHANGE 
```

## Step 3: Create wallet

Now we'll need a function to actually create the BDK wallet. To do that, the main thing we need is that wallet descriptor we got from our Coldcard, also known as an "output script descriptor" or "output descriptor." So perhaps this is a good time to explain what that actually means.

## Step 3a: What's a descriptor

Output descriptors, as [defined by Bitcoin Core](https://github.com/bitcoin/bitcoin/blob/master/doc/descriptors.md) (I'm not sure why there isn't a BIP for this), are a simple language for describing a collection of output scripts.

Here's how they're explained in the `rust-miniscript` documentation (BDK relies on rust-miniscript for parsing, serializing, and operating on descriptors):

> While spending policies in Bitcoin are entirely defined by Script; there are multiple ways of embedding these Scripts in transaction outputs; for example, P2SH or Segwit v0. These different embeddings are expressed by Output Descriptors, [which are described here](https://github.com/bitcoin/bitcoin/blob/master/doc/descriptors.md).

Here's the world's simplest descriptor, just to give you an idea what we're trying to accomplish:

```
pk(0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798)
```

That's a P2PK output with the public key inside the parenthesis. Armed with this knowledge, and of course a private key to sign the transaction, we know how to create a transaction to spend the funds at this output.

Of course it would be nice to build a wallet that's more than just a bag of random pubkeys. A typical modern wallet uses an xpub for deterministically generating new addresses, all of which can be spent by a single private key — as long as you know the "derivation path" for how the addresses were derived. Descriptors can help describe this derivation path in a cross-wallet compatible way. 

Without storing a descriptor about how a wallet derives addresses you end up with the mess over at [walletsrecovery.org](https://walletsrecovery.org/)! Hopefully storing a wallet descriptor in addition to your private key will become common practice, especially for fancier setups like multisig.

I'm a pretty visual person so let's break this language down visually. Here's a sample descriptor exported from a Coldcard:

```
wpkh([0f056943/84h/1h/0h]tpubDC7jGaaSE66Pn4dgtbAAstde4bCyhSUs4r3P8WhMVvPByvcRrzrwqSvpF9Ghx83Z1LfVugGRrSBko5UEKELCz9HoMv5qKmGq3fqnnbS5E9r/0/*)#erexmnep
```

`wpkh` = the script type (wpkh = witness public key hash = native segwit)

`0f056943` = master key fingerprint

`84h` = "purpose" part of the path

`1` = coin_type (1 is testnet, 0 would be bitcoin) 

`h` = hardened (means you can't prove a child pubkey is linked to a parent pubkey)

`0h` = account #

`tpubDC7jGaa....` = the actual xpub

`/0` = bool for change address

`*` = address index. the actual part of the path the wallet will iterate

`#erexmnep` = a checksum of the preceding string

Here's my attempt at a plain English translation of what this is saying:

> I'm making a native segwit wallet.
> I started with a master key with a fingerprint of 0f056943.
> Because this is native segwit I'll use the BIP84 derivation scheme.
> I'm on testnet.
> I'm picked "0" when Coldcard asked my what my account number is.
> Here is the actual Xpub that key 0f056943 generated at the 84h/1h/0h derivation path.
> This is not a change address.

If this is still stressing you out, one nice think about using BDK and the underlying `rust-bitcoin` library is that there are a lot of mistakes you can make that are now compile time errors. And since all your strings — like addresses and descriptors — will be parsed into Rust types like `Address` and `Descriptor`, malformed values will usually throw a helpful error. For instance, if you try to build a descriptor string by hand and get some aspect wrong there's a good chance that will let you know what's up. 

Some more reference on descriptors can be found on [BDK's website](https://bitcoindevkit.org/descriptors/).

## Step 3b: Actually create the wallet

Now that we're armed with SO MUCH knowledge about the meaning of the descriptor string we're about to pass to BDK, let's go ahead and pass the descriptor and change descriptor we got from our parsed CLI args into BDK.

At the top of the file let's import all our other dependencies, just so we don't stress about it later:

```rust
use std::str::FromStr;

use bdk::{
    bitcoin::{
        self,
        consensus::{deserialize, encode::serialize},
        util::psbt::PartiallySignedTransaction,
        Address,
    },
    blockchain::{noop_progress, ElectrumBlockchain},
    database::MemoryDatabase,
    descriptor::Descriptor,
    electrum_client::Client,
    miniscript::DescriptorPublicKey,
    wallet::{coin_selection::DefaultCoinSelectionAlgorithm, AddressIndex, AddressInfo},
    SignOptions, Wallet,
};
```

Now make the `create_wallet` function:

```rust
// Hardcoded blockchain and database types. Could also use AnyBlockchain / AnyDatabase to allow switching.
fn create_wallet(
    desc_string: &str,
    change_desc: Option<&str>,
) -> Result<Wallet<ElectrumBlockchain, MemoryDatabase>> {
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
```

To just create and use the wallet you don't need to know the precise types, but because we're spinning this out into its own function I need some type annotations. This is a blessing and curse of strongly typed languages like Rust, and Rust is about as picky as they come. The blessing is it's hard to mistakenly put the square peg in the round hole, the curse is you need to learn the precise name for each shape of data in all but the most straightforward of cases.

In this case, BDK's `Wallet` type is generic over the blockchain backend (in this case I'm choosing `ElectrumBlockchain`) and the local database for storing the wallet's state (I'm using an ephemeral `MemoryDatabase`). `bdk-cli` actually uses `AnyBlockchain` so it can swap between different backends.

A good portion of the actual logic of BDK happens in the specific database and blockchain implementations. BDK provides a nice and consistent interface via [Rust traits](https://doc.rust-lang.org/book/ch10-02-traits.html) so I, the humble frontend wallet dev, don't have to be too married to a specific infrastructure.

## Step 4: Get the balance

Alright! Now that we know how to create a wallet, let's use it.

In our `main` function we'll stop printing out our `mode` struct and instead pass it to a big match statement called `execute`.

```rust
fn main() {

    //... 

    match execute(mode) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {}.", e);
            std::process::exit(1);
        }
    }
}
```

The `execute` function looks like this:

```rust
fn execute(mode: Mode) -> Result<()> {
    match mode {
        Mode::Balance {
            descriptor,
            change_descriptor,
        } => {
            unimplemented!();
        }
        Mode::Receive { descriptor, index } => {
            unimplemented!();
        }
        Mode::Send {
            descriptor,
            change_descriptor,
            amount,
            destination,
        } => {
            unimplemented!();
        }
        Mode::Broadcast { descriptor, psbt } => {
            unimplemented!();
        }
    }
}
```

From here on out we're just filling in those match arms. Starting with `Balance`:


```rust
Mode::Balance {
    descriptor,
    change_descriptor,
} => {
    // We need to include the change descriptor to correctly calculate the balance, in case it's holding some of our sats
    let wallet = create_wallet(&descriptor, Some(&change_descriptor))?;

    // Get the balance in sats
    // It's a sum of the unspent outputs known to the wallet's internal database (so you need to sync first)
    let balance = wallet.get_balance()?;
    println!("{} sats", balance);

    // List unspent ouputs
    println!("{:#?}", wallet.list_unspent());

    Ok(())
}
```

Bitcoin is UTXO-based, not account based. That means a wallet "balance" is an abstraction. In reality, the wallet creates a list of UTXOs it knows about (and / or asks the backing blockchain if it knows of any UTXOs for a range of addresses derived from our descriptor), then sums all of the unspent amounts to generate a balance. If we didn't sync our wallet during the `create_wallet` step, we'd get a result of 0. 

## Step 5: Receive

Our output descriptor gives us all the information we need to generate receive addresses. In fact, we could easily pick an address ourselves if we wanted to: instead of passing the "\*" wildcard at the end of the derivation path, we could instead pass a specific index and build a wallet based on that particular pubkey. 

But let's have BDK do that for us.

```rust
  Mode::Receive { descriptor, index } => {
      let wallet = create_wallet(&descriptor, None)?;

      // Derives an address based on the wallet's descriptor and the given index
      let info = wallet.get_address(AddressIndex::Peek(index))?;

      // AddressInfo automatically derefs to and displays as an address, but it also includes the index if we need it
      let AddressInfo { index, address } = info;

      // Create a descriptor manually from the descriptor string
      let underived_desc: Descriptor<DescriptorPublicKey> =
          bdk::miniscript::Descriptor::from_str(&descriptor)?;

      println!("underived descriptor: {}", underived_desc);

      // Now we can derive a descriptor of the specific index.
      // We can use this with hwi's `displayaddress` method
      let desc: Descriptor<DescriptorPublicKey> = underived_desc.derive(index);

      // We could use rust-hwi to verify this address from within our "app"
      // But let's just do it manually for now
      // hwi -t "coldcard" displayaddress --desc "..."
      println!("derived descriptor: {}", desc);
      println!("index: {}", index);
      println!("address: {}", address);

      Ok(())
  }
```

BDK has a few strategies it can use for index selection. Since our wallet is stateless (we're regenerating it every time we run the command line) it makes the most to pass the index explicitly. But if you're building a stateful wallet you'll probably want to use `AddressIndex::New` or `AddressIndex::LastUnused`.

## Step 5a (optional): Verify receive address

Just to double check that BDK isn't lying to us — or, ideally, to help our users verify that we aren't lying to them — we can verify that the address we're showing for this derivation path matches with what our hardware wallet shows at that derivation path.

Here's how to do that with HWI:

```bash
hwi -t "coldcard" displayaddress --desc $DERIVEDDESC
```

If the address that HWI returns, and the Coldcard displays, and our cli wallet derived, all match then we're doing a good job!

The Coldcard's `bitcoin-core.txt` also has sample addresses from the first few child indexes if you'd like to check against those.

## Step 6: Send

To build a Bitcoin transaction you need input(s) and output(s). It turns out that "inputs" are just outputs that are already on the blockchain but unspent, hence the term "Unspent Transaction Output."

BDK lets us explicitly list the UTXOs we want to spend from, or it can use one of its built-in coin selection algorithms to pick the UTXOs for us.

```rust
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
      // You don't need to pass this `DefaultCoinSelectionAlgorithm`
      // (which is an alias for `LargestFirstCoinSelection`)
      // Just showing there's room for customization
      let mut tx_builder = wallet
          .build_tx()
          .coin_selection(DefaultCoinSelectionAlgorithm::default());

      // The Coldcard requires an output redeem witness script
      tx_builder.include_output_redeem_witness_script();

      // Enable signaling replace-by-fee
      tx_builder.enable_rbf();

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
```

When bitcoiners praise or shame wallets for their "coin control" features, this is what they're talking about. It's really a UI task. All the UTXOs are right there, just need a smart way to label and use them privately. For this simple demo wallet, however, we're just going to use BDK's `DefaultCoinSelectionAlgorithm`. 

There are a million ways to create a transaction, BDK also works to offer sane defaults to prevent known issues. For example:

* [Use non_witness_utxo when making SegWit signatures to mitigate the "SegWit bug"](https://github.com/bitcoindevkit/bdk/pull/333)

## Step 6a: What's a PSBT?

When we finalize the transaction (`tx_builder.finish()`) we get two return values, `psbt`, and `details`. Details is just what it sounds like. But what's a PSBT?

Here's how BIP 174 describes the PSBT format:

> This document proposes a binary transaction format which contains the information necessary for a signer to produce signatures for the transaction and holds the signatures for an input while the input does not have a complete set of signatures. The signer can be offline as all necessary information will be provided in the transaction. 

Just like how descriptors are a standard way to describe an output script (and therefore a wallet), a PSBT is a standard way to describe a Bitcoin transaction, even if it hasn't yet been signed completely.

(Not to be confusing, but a fully signed PSBT is also called a "PSBT" because the format is the same whether or not it's signed.)

To serialize this PSBT as a string that we can easily pass to a Coldcard as a .txt file, we'll use the base64 library to encode it.

```rust
println!("{}", base64::encode(&serialize(&psbt)));
```

Of course to test this out you'll need some testnet bitcoins to spend. It shouldn't be too hard to get some tbtc sent your wallet (you already know how to generate receive addresses after all!) but if you don't want to bother with faucets or bugging a dev you can always set up a regtest environment. I've had a great time using [`nigiri`](https://github.com/vulpemventures/nigiri) as an all-in-one bitcoin regtest node and electrum explorer. Other than the fact that I'm a web developer and nigiri takes up port 3000. Another option that a lot of the BDK devs use is [bitcoin-regtest-box](https://github.com/bitcoindevkit/bitcoin-regtest-box).

Once you have some fake sats to spend:

```bash
cargo run -- send $DESC --change $CHANGE --amount 69420 --dest $RECV
```

This should spit out a very ugly looking string of text that represents the base64-encoded psbt. Now you can send that to your hardware wallet for signing:

```bash
hwi -t "coldcard" signtx $PSBT
```

## Step 7: Broadcast

Calling the previous step "send" is a minor misnomer: we only created and signed transaction. We still need to tell the whole world about it. 

```rust
  Mode::Broadcast { descriptor, psbt } => {
      let wallet = create_wallet(&descriptor, None)?;

      // Deserialize the psbt. First as a Vec of bytes, then as a strongly typed `PartiallySignedTransaction`
      let psbt = base64::decode(&psbt)?;
      let mut psbt: PartiallySignedTransaction = deserialize(&psbt)?;

      // Uncomment this if you want a very verbose printout of everything in the psbt
      // dbg!(psbt.clone());

      // Sane default options for finalizing the transaction
      let sign_options = SignOptions::default();

      // Under the hood this uses `rust-bitcoin`'s psbt utilities to finalize the scriptSig and scriptWitness
      let _psbt_is_finalized = wallet.finalize_psbt(&mut psbt, sign_options)?;

      // Get the transaction out of the PSBT so we can broadcast it
      let tx = psbt.extract_tx();

      // Broadcast the transaction using our chosen backend, returning a `Txid` or an error
      let txid = wallet.broadcast(tx)?;

      println!("{:#?}", txid);

      Ok(())
  }
```

BDK has key management features and can sign transactions, but in this case we're just using the default `SignOptions` so we can finalize the PSBT (our Coldcard provided the only signature the PSBT needed, but that's different than "finalizing" the PSBT). The lifecycle of our PSBT is now complete.

`SignOptions` is also another example of BDK offering sane defaults:

* [Add an option to explicitly allow using non-ALL sighashes](https://github.com/bitcoindevkit/bdk/pull/353)

Now let's actually broadcast the transaction:

```bash
cargo run -- broadcast $DESC --psbt $SIGNEDPSBT 
```

If everything went well, you should now have a transaction in the Testnet blockchain (or on your local regtest, if you went that route). If you went with testnet you can plug in your txid into a testnet block explorer like [mempool.space](https://mempool.space/testnet/).

## Step 8: Good job

We're done! Celebration counts as a step.

If something seems wrong or isn't working for you please open an issue. There's also a nice little community over at the [BDK Discord](https://discord.gg/d7NkDKm).
