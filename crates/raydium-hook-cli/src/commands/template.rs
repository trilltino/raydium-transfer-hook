//! `template id | publish | show`: the optional template descriptor standard. A descriptor is
//! metadata only and never permission; see `programs/hook-template-registry`.

use std::fs;

use hook_template_sdk::{
    assess, descriptor_address, manifest_hash, publish, template_id, Descriptor, TrustFacts,
};
use raydium_hook_driver::{chain::Chain, inspect_program, RpcChain, UpgradeInfo};
use serde_json::Value;
use solana_sdk::{signature::Keypair, signature::Signer};

use super::{explorer, load_env, rpc_chain};
use crate::args::{Flags, Res};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The manifest at `path`: its parsed value, its content-derived id and the hash of its bytes.
fn read_manifest(path: &str) -> Res<(Value, [u8; 32], [u8; 32])> {
    let bytes = fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{path} is not JSON: {e}"))?;
    let id = template_id(&value).map_err(|e| format!("{path}: {e}"))?;
    Ok((value, id, manifest_hash(&bytes)))
}

/// `template id MANIFEST.json`
pub(crate) fn id(flags: &Flags) -> Res<()> {
    let path = flags.first_positional("a manifest file")?;
    let (_, id, hash) = read_manifest(path)?;
    println!("template id    {}", hex(&id));
    println!("manifest hash  {}", hex(&hash));
    println!(
        "\nThe id is SHA-256 of the canonical manifest, so it is the same wherever it is computed."
    );
    println!("The hash is of this file's exact bytes, to check a copy.");
    Ok(())
}

/// `template publish --env FILE --keypair FILE --registry PROGRAM --hook PROGRAM --manifest FILE`
pub(crate) async fn publish_descriptor(flags: &Flags) -> Res<()> {
    let (_, env) = load_env(flags)?;
    let mut chain = rpc_chain(&env, flags)?;
    let registry = flags.pubkey("registry")?;
    let hook = flags.pubkey("hook")?;
    let (_, id, hash) = read_manifest(flags.need("manifest")?)?;
    let publisher = chain.payer().pubkey();
    let ix = publish(
        &registry,
        &publisher,
        &hook,
        id,
        hash,
        flags.number("flags", 0u64)?,
    );
    let sent = chain
        .send(&[ix], &[])
        .await
        .map_err(|e| format!("publishing failed: {e}"))?;
    let address = descriptor_address(&registry, &hook, &id, &publisher).0;
    println!(
        "published descriptor {address}\n  {}",
        explorer(&env, &sent.signature)
    );
    println!(
        "\nA descriptor is metadata. It does not allow, approve or vouch for the hook, and anyone can\npublish one for any program."
    );
    Ok(())
}

/// `template show --rpc URL --registry PROGRAM --hook PROGRAM --manifest FILE --publisher KEY`
pub(crate) async fn show(flags: &Flags) -> Res<()> {
    let rpc = flags.need("rpc")?;
    let registry = flags.pubkey("registry")?;
    let hook = flags.pubkey("hook")?;
    let publisher = flags.pubkey("publisher")?;
    let (_, id, hash) = read_manifest(flags.need("manifest")?)?;
    let chain = RpcChain::new(rpc.to_string(), Keypair::new());
    let reader = chain.reader();
    let address = descriptor_address(&registry, &hook, &id, &publisher).0;
    let Some(account) = reader(address).await.map_err(|e| e.to_string())? else {
        println!("no descriptor at {address}: that publisher has not published this template for this hook");
        return Ok(());
    };
    let descriptor = Descriptor::decode(&account.data).map_err(|e| format!("{e:?}"))?;
    println!("descriptor        {address}");
    println!("hook program      {}", descriptor.hook_program);
    println!("template id       {}", hex(&descriptor.template_id));
    println!(
        "manifest hash     {} ({} the file you gave)",
        hex(&descriptor.manifest_hash),
        if descriptor.manifest_hash == hash {
            "matches"
        } else {
            "DIFFERS from"
        }
    );
    println!("publisher         {}", descriptor.template_authority);
    println!(
        "publisher flags   {:#x} (the publisher's own words, never evidence)",
        descriptor.flags
    );

    let program = inspect_program(&reader, hook)
        .await
        .map_err(|e| e.to_string())?;
    let upgrade_authority = match program.upgrade {
        Some(UpgradeInfo::Authority(a)) => Some(a),
        _ => None,
    };
    let assessment = assess(
        &descriptor,
        &TrustFacts {
            hook_upgrade_authority: upgrade_authority,
            ..Default::default()
        },
    );
    println!(
        "\nhook upgrade authority  {}",
        match program.upgrade {
            Some(UpgradeInfo::Authority(a)) => a.to_string(),
            Some(UpgradeInfo::Immutable) => "none: the program is immutable".into(),
            Some(UpgradeInfo::NotApplicable(l)) => format!("not applicable (loader {l})"),
            None => "unknown".into(),
        }
    );
    println!("assessment              {}", assessment.label());
    println!(
        "  author-verified: {}  (the publisher is the hook's upgrade authority)\n  repository-tested and audited: not derivable from a descriptor",
        assessment.author_verified
    );
    Ok(())
}
