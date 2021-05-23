# Building a CLI Bitcoin wallet with BDK and Rust

Today we're going to build a Bitcoin wallet. More specifically, a descriptor-based, watch-only wallet that can create PSBTs to be signed offline by a Coldcard, and broadcast those PSBTs once finalized.

If that sounds like a lot of novel concepts, you've come to the right place, because explaining and understanding these concepts is the whole point of this guide.

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

BDK uses these libraries for the all-important TKTK bitcoin stuff, while BDK handles:

1. Connecting to a blockchain backend like Electrum
2. Storing the wallet state in a database 
3. All the things wallets need TKTK what's a good dileneation

Because BDK is written in Rust it's extremely cross platform, including Web Assembly. Also, language bindings are in the works so you can use BDK from other languages like Python or Java.

## Let's start coding though

## Step 1: Project setup
First create a new binary rust project and add these dependencies to Cargo.toml:

```
anyhow = "1.0.40"
base64 = "0.13.0"
bdk = "0.6.0"
pico-args = "0.4.0"
```

If you're absolutely new to Rust and what I said doesn't make any sense, the [official Rust Programming Language book](https://doc.rust-lang.org/stable/book/ch01-00-getting-started.html) is highly recommended.

## Step 2: Argument parsing

To kick off this project we'll want to parse some command line arguments. Because this isn't the main focus I'm going to speed through this. 

I'm using [pico-args](https://github.com/RazrFalcon/pico-args) for argument parsing and [anyhow](https://github.com/dtolnay/anyhow) for easy error handling.

TODO: inline comments for everything 
```rust
use std::str::FromStr;

use anyhow::{bail, ensure, Context, Result};

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
        amount: u64,
        destination: String,
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
            amount: pargs.value_from_str("--amount").context("Missing amount")?,
            destination: pargs
                .value_from_str("--dest")
                .context("Missing destination address")?,
        },
        _ => bail!("Unknown mode"),
    };

    Ok(info)
}
```

## Step 3: Create wallet

Now we'll need a function to actually create the BDK wallet. To create a BDK wallet the main thing you need is a wallet descriptor, also known as an "output script descriptor" or "output descriptor." So perhaps this is a good time to explain what that actually means.

## Step 3a: What's a descriptor

Output descriptors, as [defined by Bitcoin Core](https://github.com/bitcoin/bitcoin/blob/master/doc/descriptors.md) (TKTK why not a BIP for these?), are a simple language for describing a collection output scripts.

Here's the world's simplest descriptor, just to give you an idea what we're trying to accomplish:

```
pk(0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798)
```

That's a P2PK output with the public key inside the parenthesis. Armed with this knowledge, and of course a private key to sign the transaction, we know how to create a transaction to spend the funds at this output.

Of course these days wallets are a whole lot fancier than just a bag of random pubkeys, and that's what descriptors help describe.

I'm a pretty visual person so let's break this language down visually. Here's a sample descriptor exported from a Coldcard:

```
hwi -t "coldcard" --chain test getdescriptors
```

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
`#erexmnep` = a checksum 

Here's my attempt at a plain English translation of what this is saying:

I'm making a native segwit wallet.
I started with a master key with a fingerprint of 0f056943.
Because this is native segwit I'll use the BIP84 derivation scheme.
I'm on testnet.
I'm picked "0" when Colcard asked my what my account number is.
Here is the actual Xpub that key 0f056943 generated at the 84h/1h/0h derivation path.
This is not a change address.

It's important to remember that your 24 word private key isn't everything you need to know to access your funds. That's why [walletsrecovery.org](https://walletsrecovery.org/) exists! Hopefully storing a wallet descriptor in addition to your private key will become common practice, especially for fancier setups like multisig.

## Step 3b: Actually create the wallet

Now that we're armed with SO MUCH knowledge about the meaning of the descriptor string we're about to pass to BDK, let's go ahead and pass it.

```
fn create_wallet(desc_string: String) -> Result<Wallet<ElectrumBlockchain, MemoryDatabase>> {
    let client = Client::new("ssl://electrum.blockstream.info:60002")?;
    let wallet = Wallet::new(
        desc_string.as_str(),
        None,
        bitcoin::Network::Testnet,
        MemoryDatabase::default(),
        ElectrumBlockchain::from(client),
    )?;

    Ok(wallet)
}
```

The second argument to `Wallet::new` is an optional change descriptor. I'll leave that as an exercise for the reader.

Something that I like about BDK is that this wallet creation will fail with incorrect values. TKTK checksum, hardened/non-hardened, testnet/bitcoin.

## Step 4: Get the balance

Bitcoin is UTXO-based, not account based. That means a wallet "balance" is an abstraction. In reality, the wallet needs to scan the blockchain for UTXOs it might own, and given the size of the Bitcoin blockchain, and the basically infinite number of pubkeys you can generate from one xpub, we'll need some help and some heuristics.

TKTK where is BDK actually asking the blockchain backend for UTXOs? I just see batch adddress generation

```
Mode::Balance { descriptor } => { ...}

```

## Step 5: Receive

Our output descriptor gives us all the information we need to generate receive addresses. In fact, we could easily pick an address ourselves if we wanted to: instead of passing the "\*" wildcard at the end of the derivation path, we could instead pass a specific index and build a wallet based on that particular pubkey. 

But let's have BDK do that for us.

```
Mode::Receive { descriptor, index } => {...}
```

BDK has a few strategies it can use for index selection. Since our wallet is stateless (we're regenerating it every time we run the command line) it makes the most to pass the index explicitly. But if you're building a stateful wallet you'll probably want to use `AddressIndex::New` or `AddressIndex::LastUnused`.

## Step 5a (optional): Verify receive address

TKTK Verify the address on your Coldcard.

Need to figure out the derivation path???

## Step 6: Send

To build a Bitcoin transaction you need input(s) and output(s). We're already getting to know outputs pretty well TKTK lol is this true idk. And it turns out that "inputs" are just outputs that are already on the blockchain but unspent, hence the term "Unspent Transaction Output."

BDK lets us explicitly list the UTXOs we want to spend from, or it can use one of its built-in coin selection algorithms to pick the UTXOs for you.

```
Mode::Send...
```

## Step 6a: Building the transaction

```
let dest_script = Address::from_str(destination.as_str())
                .unwrap()
                .script_pubkey();

            let mut tx_builder = wallet.build_tx();

            tx_builder.add_recipient(dest_script, amount);

            let (psbt, details) = tx_builder.finish()?;
            println!("{:#?}", details);



```

## Step 6b: What's a PSBT?

When we finalize the transaction (`tx_builder.finish()`) we get two return values, `psbt`, and `details`. Details is just what it sounds like. But what's a PSBT?

Here's how BIP 174 describes the PSBT format:

> This document proposes a binary transaction format which contains the information necessary for a signer to produce signatures for the transaction and holds the signatures for an input while the input does not have a complete set of signatures. The signer can be offline as all necessary information will be provided in the transaction. 

Just like how descriptors are a standard way to describe an output script (and therefore a wallet), a PSBT is a standard way to describe a Bitcoin transaction, even if it hasn't yet been signed completely.

(Not to be confusing, but a fully signed PSBT is also called a "PSBT" because the format is the same whether or not it's signed.)


To serialize this PSBT as a string that we can easily pass to a Coldcard as a .txt file, we'll use the base64 library to encode it.

```
            println!("{}", base64::encode(&serialize(&psbt)));
```

## Step 7: Broadcast

TKTK why do we need a "wallet" to send a tx other than the fact that it has a client?

## Step 7a: Sign the transaction 

TKTK sign the tx on ur Coldcard

```
hwi -t "coldcard" --chain test signtx <psbt>
```

## Step 7b: Parse and broadcast the transaction

```
cargo run -- broadcast $DESC --psbt cHNidP8BAHEBAAAAAR9TFhoj4PG4z2/B8qNATCJ0CrJeOw+dtVbtsRSlCKukAQAAAAD/////AmiTAAAAAAAAFgAUGrXZLeR+7Hyak/yY0LHXH1TrvgdbLwAAAAAAABYAFBq12S3kfux8mpP8mNCx1x9U674HAAAAAAABAR9QwwAAAAAAABYAFLmhqH5QkSw0OFYQc3WCYUrx4xwTIgID7J1BU5aMkSBXcNgjcDStPQdhEljwOJUO0smoIPyMqtFIMEUCIQDjTTX1sgSsFCutP5Pf3HgotpnoB+GNjvVoKJJtsjBwyAIgMA8OR+xT/mJpt0jxlY4eeTDyg5d4uT7/VKTW1bhyZt8BAQMEAQAAACIGA+ydQVOWjJEgV3DYI3A0rT0HYRJY8DiVDtLJqCD8jKrRGFgG+ZhUAACAAQAAgAAAAIAAAAAAAAAAAAAiAgMkc358pN8sztyQHnyQaHdlHv6Lqv1KCjbpIe7vVAHd4RhYBvmYVAAAgAEAAIAAAACAAAAAAAEAAAAAIgIDJHN+fKTfLM7ckB58kGh3ZR7+i6r9Sgo26SHu71QB3eEYWAb5mFQAAIABAACAAAAAgAAAAAABAAAAAA==
```

```
Error: Electrum(Protocol(String("sendrawtransaction RPC error: {\"code\":-26,\"message\":\"non-mandatory-script-verify-flag (Witness program hash mismatch)\"}"))).
```




