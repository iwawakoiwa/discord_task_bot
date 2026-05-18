mod commands;
mod db;
mod models;
mod reminder;

use std::sync::Arc;

use poise::serenity_prelude as serenity;

#[derive(Debug)]
pub struct Data {
    pub db: db::Database,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN が設定されていません");
    let db_path = std::env::var("DATABASE_URL").unwrap_or_else(|_| "tasks.db".to_string());

    let database = db::Database::new(&db_path).expect("データベースのオープンに失敗しました");
    database.init().expect("データベーススキーマの初期化に失敗しました");

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![commands::task(), commands::now(), commands::help()],
            on_error: |err| {
                Box::pin(async move {
                    match err {
                        poise::FrameworkError::Command { error, ctx, .. } => {
                            eprintln!("コマンドエラー: {:?}", error);
                            let _ = ctx
                                .send(
                                    poise::CreateReply::default()
                                        .ephemeral(true)
                                        .content(format!("エラーが発生しました: {}", error)),
                                )
                                .await;
                        }
                        err => eprintln!("フレームワークエラー: {:?}", err),
                    }
                })
            },
            ..Default::default()
        })
        .setup(|ctx, ready, framework| {
            Box::pin(async move {
                println!("ボットが起動しました: {}", ready.user.name);
                let commands = &framework.options().commands;
                if let Ok(guild_id_str) = std::env::var("GUILD_ID") {
                    let guild_id = guild_id_str
                        .parse::<u64>()
                        .expect("GUILD_ID が無効な数値です");
                    poise::builtins::register_globally::<Data, Error>(ctx, &[]).await?;
                    poise::builtins::register_in_guild(
                        ctx,
                        commands,
                        serenity::GuildId::new(guild_id),
                    )
                    .await?;
                    println!("コマンドをギルド {} に登録しました（即時反映）", guild_id);
                } else {
                    poise::builtins::register_globally(ctx, commands).await?;
                    println!("コマンドをグローバルに登録しました（反映まで最大1時間）");
                }
                Ok(Data { db: database })
            })
        })
        .build();

    let intents = serenity::GatewayIntents::non_privileged();

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .expect("Discord クライアントの作成に失敗しました");

    // リマインダーバックグラウンドタスク（60秒ごとにチェック）
    let db_for_reminder = {
        // Data はフレームワーク内にあるので、別途 DB を開く
        let db_path = std::env::var("DATABASE_URL").unwrap_or_else(|_| "tasks.db".to_string());
        db::Database::new(&db_path).expect("リマインダー用DBのオープンに失敗しました")
    };
    let http = Arc::clone(&client.http);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = reminder::check_reminders(&db_for_reminder, &http).await {
                eprintln!("リマインダーチェックエラー: {:?}", e);
            }
        }
    });

    client.start().await.expect("ボットの起動に失敗しました");
}
