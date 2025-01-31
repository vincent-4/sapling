/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod merge;
mod pushredirection;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

use self::merge::MergeArgs;
use self::pushredirection::PushRedirectionArgs;

/// Manage megarepo
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: MegarepoSubcommand,
}

#[derive(Subcommand)]
enum MegarepoSubcommand {
    /// Manage which repos are pushredirected to the large repo
    PushRedirection(PushRedirectionArgs),
    Merge(MergeArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    match args.subcommand {
        MegarepoSubcommand::PushRedirection(args) => pushredirection::run(&ctx, app, args).await?,
        MegarepoSubcommand::Merge(args) => merge::run(&ctx, app, args).await?,
    }

    Ok(())
}
