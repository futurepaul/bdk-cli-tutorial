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

BDK uses these libraries for the all-important TKTK bitcoin stuff, while BDK handles:

1. Connecting to a blockchain backend like Electrum
2. Storing the wallet state in a database 
3. All the things wallets need TKTK what's a good dileneation

Because BDK is written in Rust it's extremely cross platform, including Web Assembly. Also, language bindings are in the works so you can use BDK from other languages like Python or Java.

## Let's start coding though

## Step 1a: Project setup
First create a new binary rust project and add these dependencies to Cargo.toml:

```toml
anyhow = "1.0.40"
base64 = "0.13.0"
bdk = "0.6.0"
pico-args = "0.4.0"
```

If you're absolutely new to Rust and what I said doesn't make any sense, the [official Rust Programming Language book](https://doc.rust-lang.org/stable/book/ch01-00-getting-started.html) is highly recommended.

## Setup 1b: Get your descriptors

If you installed HWI you should be able to run `hwi enumerate` and see a list of connected hardware wallets. If you see a wallet you can reference it by type and ask for a descriptor. In this case I'm asking for a Testnet descriptor.

```bash
hwi -t "coldcard" --chain test getdescriptors
```

That should output an array of "receive" descriptors and an array of "internal" descriptors. Grab the one from each list that starts with `wpkh`.

If you don't want to bother with getting HWI setup you can also export your Coldcard's descriptors to a .txt file and transfer it via microSD card:

`Advanced > MicroSD Card > Export Wallet > Bitcoin Core`

And if you don't have a spare hardware wallet you can run a [Coldcard simulator], or you can grab the sample descriptors from [this GitHub issue](https://github.com/Coldcard/firmware/pull/32) (though you won't be able to sign them for obvious reasons).

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
#[derive(Debug, Clone)]
enum Mode {
  ...
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
  ...
}
```

Now we can kick the tires of our various subcommands.

```bash
cargo run -- balance $DESC
cargo run -- receive $DESC --index 123
cargo run -- send $DESC --change $CHANGE --amount 12345 --dest $RECV
cargo run -- broadcast $DESC --psbt abcdefg
```

Everything after the first `--` are arguments we're passing to our program, before the `--` are arguments we're passing to Rust's Cargo build tool.

## Step 3: Create wallet

Now we'll need a function to actually create the BDK wallet. To create a BDK wallet the main thing you need is a wallet descriptor, also known as an "output script descriptor" or "output descriptor." So perhaps this is a good time to explain what that actually means.

## Step 3a: What's a descriptor

Output descriptors, as [defined by Bitcoin Core](https://github.com/bitcoin/bitcoin/blob/master/doc/descriptors.md) (TKTK why not a BIP for these?), are a simple language for describing a collection of output scripts.

Here's how they're explained in the `rust-miniscript` documentation (BDK relies on rust-miniscript for parsing, serializing, and operating on descriptors):

> While spending policies in Bitcoin are entirely defined by Script; there are multiple ways of embedding these Scripts in transaction outputs; for example, P2SH or Segwit v0. These different embeddings are expressed by Output Descriptors, [which are described here](https://github.com/bitcoin/bitcoin/blob/master/doc/descriptors.md).

(TKTK will there be a new descriptor for p2tr? or will it still be wpkh?)

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
> Guard me from typos please. (TKTK is this what the checksum actually does? apparently the checksum is diff for v1 of segwit vs v0?)

## Step 3b: Actually create the wallet

Now that we're armed with SO MUCH knowledge about the meaning of the descriptor string we're about to pass to BDK, let's go ahead and pass the descriptor and change descriptor we got from our parsed CLI args.

```rust
fn create_wallet(desc_string: String) -> Result<Wallet<ElectrumBlockchain, MemoryDatabase>> {
   ... 
}
```

To just create and use the wallet you don't need to know the precise types, but because we're spinning this out into its own function I need some type annotations. This is a blessing and curse of strongly typed languages like Rust, and Rust is about as picky as they come. The blessing is it's hard to mistakenly put the square peg in the round hole, the curse is you need to learn the precise name for each shape of data in all but the most straightforward of cases.

In this case, BDK's `Wallet` type is generic over the blockchain backend (in this case I'm choosing `ElectrumBlockchain`) and the local database for storing the wallet's state (I'm using an ephemeral `MemoryDatabase`). I wasn't born knowing the names for those things, I had to look them up. TKTK documentation?

A good portion of the actual logic of BDK happens in the specific database and blockchain implementations. BDK provides a nice and consistent interface so I, the humble frontend wallet dev, don't have to worry too much about the specifics.

TKTK: Something that I like about BDK is that this wallet creation will fail with incorrect values. TKTK checksum, hardened/non-hardened, testnet/bitcoin.

## Step 4: Get the balance

Alright! Now that we know how to create a wallet, let's use it.

```rust
Mode::Balance { descriptor } => { ...}
```

Bitcoin is UTXO-based, not account based. That means a wallet "balance" is an abstraction. In reality, the wallet needs to scan the blockchain for UTXOs it might own, and given the size of the Bitcoin blockchain, and the basically infinite number of pubkeys you can generate from one xpub, we'll need some help and some heuristics.

TKTK where is BDK actually asking the blockchain backend for UTXOs? I just see batch adddress generation

## Step 5: Receive

Our output descriptor gives us all the information we need to generate receive addresses. In fact, we could easily pick an address ourselves if we wanted to: instead of passing the "\*" wildcard at the end of the derivation path, we could instead pass a specific index and build a wallet based on that particular pubkey. 

But let's have BDK do that for us.

```rust
Mode::Receive { descriptor, index } => {...}
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

To build a Bitcoin transaction you need input(s) and output(s). We're already getting to know outputs pretty well TKTK lol is this true idk. And it turns out that "inputs" are just outputs that are already on the blockchain but unspent, hence the term "Unspent Transaction Output."

BDK lets us explicitly list the UTXOs we want to spend from, or it can use one of its built-in coin selection algorithms to pick the UTXOs for you.

```rust
Mode::Send...
```

When bitcoiners praise or shame wallets for their "coin control" features, this is what they're talking about. It's really a UI task. All the UTXOs are right there, just need a smart way to label and use them privately.

TKTK anything more to say here?

## Step 6a: Building the transaction

```rust
let dest_script = ... 
```

TKTK stuck on this part 

## Step 6b: What's a PSBT?

When we finalize the transaction (`tx_builder.finish()`) we get two return values, `psbt`, and `details`. Details is just what it sounds like. But what's a PSBT?

Here's how BIP 174 describes the PSBT format:

> This document proposes a binary transaction format which contains the information necessary for a signer to produce signatures for the transaction and holds the signatures for an input while the input does not have a complete set of signatures. The signer can be offline as all necessary information will be provided in the transaction. 

Just like how descriptors are a standard way to describe an output script (and therefore a wallet), a PSBT is a standard way to describe a Bitcoin transaction, even if it hasn't yet been signed completely.

(Not to be confusing, but a fully signed PSBT is also called a "PSBT" because the format is the same whether or not it's signed.)


To serialize this PSBT as a string that we can easily pass to a Coldcard as a .txt file, we'll use the base64 library to encode it.

```rust
println!("{}", base64::encode(&serialize(&psbt)));
```

Of course to test this out you'll need some testnet bitcoins to spend. It shouldn't be too hard to get some tbtc sent your wallet (you already know how to generate receive addresses after all!) but if you don't want to bother with faucets or bugging a dev you can always set up a regtest environment. I've had a great time using [`nigiri`](https://github.com/vulpemventures/nigiri) as an all-in-one bitcoin regtest node and electrum explorer. Other than the fact that I'm a web developer and nigiri takes up port 3000.

Once you have some fake sats to spend:

```bash
cargo run -- send $DESC --change $CHANGE --amount 69420 --dest $RECV
```

This should spit out a very ugly looking string of text that represents the base64-encoded psbt. Now you can send that to your hardware wallet for signing:

```bash
hwi -t "coldcard" signtx $PSBT
```

## Step 7: Broadcast

Calling the previous step "send" is a minor misnomer: we only created a transaction. We still need to tell the whole world about it. Thankfully there's no special logic here, we just need to deserialize the signed psbt and blast it out to our Electrum client.

```rust
Mode::Broadcast { descriptor, psbt } => { ... }
```

TKTK why do we need a "wallet" to send a tx other than the fact that it has a client?

## Step 7a: Sign the transaction 

TKTK sign the tx on ur Coldcard

```bash
hwi -t "coldcard" --chain test signtx <psbt>
```

## Step 7b: Parse and broadcast the transaction


```
cargo run -- broadcast $DESC --psbt cHNidP8BAHEBAAAAAR9TFhoj4PG4z2/B8qNATCJ0CrJeOw+dtVbtsRSlCKukAQAAAAD/////AmiTAAAAAAAAFgAUGrXZLeR+7Hyak/yY0LHXH1TrvgdbLwAAAAAAABYAFBq12S3kfux8mpP8mNCx1x9U674HAAAAAAABAR9QwwAAAAAAABYAFLmhqH5QkSw0OFYQc3WCYUrx4xwTIgID7J1BU5aMkSBXcNgjcDStPQdhEljwOJUO0smoIPyMqtFIMEUCIQDjTTX1sgSsFCutP5Pf3HgotpnoB+GNjvVoKJJtsjBwyAIgMA8OR+xT/mJpt0jxlY4eeTDyg5d4uT7/VKTW1bhyZt8BAQMEAQAAACIGA+ydQVOWjJEgV3DYI3A0rT0HYRJY8DiVDtLJqCD8jKrRGFgG+ZhUAACAAQAAgAAAAIAAAAAAAAAAAAAiAgMkc358pN8sztyQHnyQaHdlHv6Lqv1KCjbpIe7vVAHd4RhYBvmYVAAAgAEAAIAAAACAAAAAAAEAAAAAIgIDJHN+fKTfLM7ckB58kGh3ZR7+i6r9Sgo26SHu71QB3eEYWAb5mFQAAIABAACAAAAAgAAAAAABAAAAAA==
```
how could bdk help me avoid this error in the first place?

```
Error: Electrum(Protocol(String("sendrawtransaction RPC error: {\"code\":-26,\"message\":\"non-mandatory-script-verify-flag (Witness program hash mismatch)\"}"))).
```




