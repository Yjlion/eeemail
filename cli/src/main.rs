//! eeemail's headless driver.
//!
//! This exists to exercise the email layer without a UI: it is what
//! integration tests drive, and what you reach for when you need to know what
//! the engine actually did. It is deliberately not a general mail client --
//! there is no interactive mode and no daemon. Every invocation opens the
//! account, does one thing, prints the result, and exits.
//!
//! Output is JSON on stdout, one document per invocation, so a test can pipe
//! it into `jq` and a human can read it. Errors go to stderr with a non-zero
//! exit.
//!
//! ```text
//! eeemail-cli <db-path> <command> [args...]
//! ```

use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use deltachat::config::Config;
use deltachat::context::{Context, ContextBuilder};
use deltachat::email;
use deltachat::message::MsgId;
use serde_json::{Value, json};

const USAGE: &str = "\
eeemail-cli <db-path> <command> [args...]

Account
  info                          every config value the engine reports
  config get <key>              read one config value
  config set <key> <value>      write one config value

Messages
  show <msg-id>                 subject, recipients, labels, crypto state
  show --raw <msg-id>           the original MIME bytes, if still retained
  thread <msg-id>               the conversation this message belongs to

Organization
  labels                        list labels
  label create <name>           create a label
  label apply <name> <msg-id>   apply a label to a message
  label remove <name> <msg-id>  remove a label from a message
  archive <msg-id>              archive a message
  unarchive <msg-id>            move a message back to the inbox
  search <text>                 search body, subject, recipients

Policy
  retention get                 raw-MIME and server retention
  retention set raw <days>      0 = off, N days, -1 = forever
  retention set server <days>   0 = delete after download, N days, -1 = never
  encryption get                strict | opportunistic | lenient
  encryption set <mode>         set the global encryption mode
  receipts get                  never | verified-only | always
  receipts set <policy>         set who gets read receipts
";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<Value> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print!("{USAGE}");
        return Ok(json!({}));
    }
    if args.len() < 2 {
        bail!("missing command\n\n{USAGE}");
    }

    let ctx = ContextBuilder::new(args[0].clone().into())
        .open()
        .await
        .with_context(|| format!("cannot open account at {}", args[0]))?;

    // eeemail's defaults differ from upstream's, and are applied at setup
    // rather than as compile-time defaults. No-op for an already-configured
    // account.
    email::policy::apply_defaults(&ctx).await?;

    let rest: Vec<&str> = args[2..].iter().map(String::as_str).collect();
    dispatch(&ctx, &args[1], &rest).await
}

async fn dispatch(ctx: &Context, command: &str, args: &[&str]) -> Result<Value> {
    match (command, args) {
        ("info", []) => {
            let info = ctx.get_info().await?;
            Ok(serde_json::to_value(info)?)
        }
        ("config", ["get", key]) => {
            let value = ctx.get_config(config_key(key)?).await?;
            Ok(json!({ "key": key, "value": value }))
        }
        ("config", ["set", key, value]) => {
            ctx.set_config(config_key(key)?, Some(value)).await?;
            Ok(json!({ "key": key, "value": value }))
        }

        ("show", ["--raw", id]) => {
            let msg_id = msg_id(id)?;
            match email::rawmime::load(ctx, msg_id).await? {
                Some(bytes) => Ok(json!({
                    "msgId": msg_id.to_u32(),
                    "bytes": bytes.len(),
                    // Lossy on purpose: mail is not required to be UTF-8, but a
                    // JSON string is. Byte-exact export is a different job.
                    "raw": String::from_utf8_lossy(&bytes),
                })),
                None => Ok(json!({
                    "msgId": msg_id.to_u32(),
                    "raw": Value::Null,
                    "note": "not retained; raw_mime_retention_days may have elapsed",
                })),
            }
        }
        ("show", [id]) => show(ctx, msg_id(id)?).await,
        ("thread", [id]) => thread(ctx, msg_id(id)?).await,

        ("labels", []) => {
            let labels = email::labels::list(ctx).await?;
            Ok(json!(
                labels
                    .into_iter()
                    .map(|l| json!({
                        "id": l.id.to_i64(),
                        "name": l.name,
                        "isSystem": l.is_system,
                    }))
                    .collect::<Vec<_>>()
            ))
        }
        ("label", ["create", name]) => {
            let label = email::labels::create(ctx, name, None).await?;
            Ok(json!({ "id": label.id.to_i64(), "name": label.name }))
        }
        ("label", ["apply", name, id]) => {
            let label = email::labels::create(ctx, name, None).await?;
            email::labels::apply(ctx, &[msg_id(id)?], label.id).await?;
            Ok(json!({ "applied": name, "msgId": msg_id(id)?.to_u32() }))
        }
        ("label", ["remove", name, id]) => {
            let label = email::labels::by_name(ctx, name)
                .await?
                .with_context(|| format!("no label named {name:?}"))?;
            email::labels::unapply(ctx, &[msg_id(id)?], label.id).await?;
            Ok(json!({ "removed": name, "msgId": msg_id(id)?.to_u32() }))
        }
        ("archive", [id]) => {
            email::labels::archive(ctx, &[msg_id(id)?]).await?;
            Ok(json!({ "archived": msg_id(id)?.to_u32() }))
        }
        ("unarchive", [id]) => {
            email::labels::unarchive(ctx, &[msg_id(id)?]).await?;
            Ok(json!({ "unarchived": msg_id(id)?.to_u32() }))
        }
        ("search", [text]) => {
            let hits = email::search::search(ctx, &email::search::SearchQuery::text(text)).await?;
            Ok(json!({
                "query": text,
                "msgIds": hits.iter().map(|id| id.to_u32()).collect::<Vec<_>>(),
            }))
        }

        ("retention", ["get"]) => Ok(json!({
            "rawMime": format!("{:?}", email::rawmime::Retention::load(ctx).await?),
            "server": format!("{:?}", email::policy::ServerRetention::load(ctx).await?),
        })),
        ("retention", ["set", "raw", days]) => {
            ctx.set_config(Config::RawMimeRetentionDays, Some(days))
                .await?;
            Ok(json!({ "rawMime": format!("{:?}", email::rawmime::Retention::load(ctx).await?) }))
        }
        ("retention", ["set", "server", days]) => {
            ctx.set_config(Config::ServerRetentionDays, Some(days))
                .await?;
            Ok(
                json!({ "server": format!("{:?}", email::policy::ServerRetention::load(ctx).await?) }),
            )
        }

        ("encryption", ["get"]) => Ok(json!({
            "mode": format!("{:?}", email::policy::EncryptionMode::load(ctx).await?),
        })),
        ("encryption", ["set", mode]) => {
            let mode = match *mode {
                "strict" => email::policy::EncryptionMode::Strict,
                "opportunistic" => email::policy::EncryptionMode::Opportunistic,
                "lenient" => email::policy::EncryptionMode::Lenient,
                other => bail!(
                    "unknown encryption mode {other:?}; expected strict, opportunistic or lenient"
                ),
            };
            email::policy::EncryptionMode::set(ctx, mode).await?;
            Ok(json!({ "mode": format!("{mode:?}") }))
        }

        ("receipts", ["get"]) => Ok(json!({
            "policy": format!("{:?}", email::receipts::MdnPolicy::load(ctx).await?),
        })),
        ("receipts", ["set", policy]) => {
            let policy = match *policy {
                "never" => email::receipts::MdnPolicy::Never,
                "verified-only" => email::receipts::MdnPolicy::VerifiedOnly,
                "always" => email::receipts::MdnPolicy::Always,
                other => bail!(
                    "unknown receipt policy {other:?}; expected never, verified-only or always"
                ),
            };
            email::receipts::MdnPolicy::set(ctx, policy).await?;
            Ok(json!({ "policy": format!("{policy:?}") }))
        }

        _ => bail!("unknown command {command:?}\n\n{USAGE}"),
    }
}

async fn show(ctx: &Context, msg_id: MsgId) -> Result<Value> {
    let msg = deltachat::message::Message::load_from_db(ctx, msg_id)
        .await
        .with_context(|| format!("no message {}", msg_id.to_u32()))?;
    let recipients = email::recipients::load(ctx, msg_id).await?;
    let labels = email::labels::of_msg(ctx, msg_id).await?;
    let crypto = email::policy::message_crypto(ctx, msg_id).await?;
    let undelivered = email::policy::undelivered(ctx, msg_id).await?;

    Ok(json!({
        "msgId": msg_id.to_u32(),
        "subject": msg.get_subject(),
        "text": msg.get_text(),
        "recipients": recipients.iter().map(|r| json!({
            "kind": format!("{:?}", r.kind),
            "addr": r.addr,
            "name": r.name,
        })).collect::<Vec<_>>(),
        "labels": labels.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "archived": email::labels::is_archived(ctx, msg_id).await?,
        "crypto": {
            "encrypted": crypto.encrypted,
            "signed": crypto.signed,
            "verified": crypto.verified,
        },
        // Almost always empty. When it is not, someone the message was
        // addressed to never received it.
        "undeliveredTo": undelivered,
        "rawMimeRetained": email::rawmime::is_retained(ctx, msg_id).await?,
    }))
}

async fn thread(ctx: &Context, msg_id: MsgId) -> Result<Value> {
    let Some(thread_id) = email::threading::thread_of(ctx, msg_id).await? else {
        return Ok(json!({ "msgId": msg_id.to_u32(), "threadId": Value::Null }));
    };
    let roots = email::threading::tree(ctx, thread_id).await?;

    // Flat, in display order, with a depth per row: that is what a reading pane
    // draws, and it avoids nesting JSON as deep as the reply chain happens to
    // be.
    let mut items = Vec::new();
    let mut stack: Vec<(email::threading::ThreadNode, u32)> =
        roots.into_iter().rev().map(|n| (n, 0)).collect();
    while let Some((node, depth)) = stack.pop() {
        let msg = deltachat::message::Message::load_from_db(ctx, node.msg_id).await?;
        items.push(json!({
            "msgId": node.msg_id.to_u32(),
            "depth": depth,
            "subject": msg.get_subject(),
        }));
        for child in node.children.into_iter().rev() {
            stack.push((child, depth.saturating_add(1)));
        }
    }
    Ok(json!({
        "msgId": msg_id.to_u32(),
        "threadId": thread_id.to_i64(),
        "messages": items,
    }))
}

fn msg_id(raw: &str) -> Result<MsgId> {
    Ok(MsgId::new(
        raw.parse()
            .with_context(|| format!("{raw:?} is not a message id"))?,
    ))
}

fn config_key(raw: &str) -> Result<Config> {
    use std::str::FromStr as _;
    Config::from_str(raw).with_context(|| format!("unknown config key {raw:?}"))
}
