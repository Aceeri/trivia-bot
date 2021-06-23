use std::{
    env,
    collections::HashMap,
    sync::{Mutex, Arc},
};

use serenity::{
    async_trait, 
    client::bridge::gateway::GatewayIntents,
    model::{
        guild::{GuildStatus, Guild, Role},
        id::{
            ChannelId,
            RoleId,
        },
        event::TypingStartEvent, 
        gateway::Ready,
        interactions::{
            ApplicationCommand,
            ApplicationCommandInteractionDataOptionValue,
            ApplicationCommandOptionType,
            Interaction,
            InteractionResponseType,
            InteractionType,
        },
    },
    utils::Colour,
    cache::Cache,
    prelude::*,
};

const PERMISSION_DENIED: &'static str = "You do not have permission to use this command and it has been reported to the local authorities. Spend your last moments repenting.";

struct Handler {
    teams: Arc<Mutex<Teams>>,
    host_role: Arc<Mutex<Option<RoleId>>>,
}

struct Teams {
    teams: HashMap<ChannelId, Team>,
}

#[derive(Debug, Clone)]
struct Team {
    role: Role,
    score: i64,
}

impl Teams {
    fn new() -> Teams {
        Teams {
            teams: HashMap::new(),
        }
    }

    fn create_team(&mut self, channel: ChannelId, role: Role) {
        self.teams.entry(channel).or_insert(Team {
            role: role,
            score: 0,
        });
    }

    fn get_team(&mut self, channel: &ChannelId) -> Option<Team> {
        self.teams.get(channel).cloned()
    }
}

impl Handler {
    fn new() -> Handler {
        Handler {
            teams: Arc::new(Mutex::new(Teams::new())),
            host_role: Arc::new(Mutex::new(None)),
        }
    }

    fn create_team(&self, channel: ChannelId, role: Role) {
        let mut teams_data = self.teams.lock().unwrap();
        teams_data.create_team(channel, role)
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if interaction.kind == InteractionType::ApplicationCommand {
            if let Some(data) = interaction.data.as_ref() {
                let content = match data.name.as_str() {
                    "ping" => "pong".to_string(),
                    "id" => {
                        let options = data
                            .options
                            .get(0)
                            .expect("Expected user option")
                            .resolved
                            .as_ref()
                            .expect("Expected user object");

                        if let ApplicationCommandInteractionDataOptionValue::User(user, _member) =
                            options
                        {
                            format!("{}'s id is {}", user.tag(), user.id)
                        } else {
                            "Please provide a valid user".to_string()
                        }
                    },
                    "team" => {
                        let suboption = data.options.get(0).expect("Expected sub option");
                        match suboption.name.as_str().clone() {
                            "rename" => {
                                let name_arg = suboption
                                    .options
                                    .get(0)
                                    .expect("Expected new team name")
                                    .resolved
                                    .as_ref()
                                    .expect("Expected string");

                                match (name_arg, interaction.channel_id) {
                                    (ApplicationCommandInteractionDataOptionValue::String(new_name), Some(channel_id)) => {
                                        {
                                            let teams = self.teams.lock().unwrap().get_team(&channel_id);
                                            match teams {
                                                Some(team) => {
                                                    match team.role.edit(ctx.http.clone(), |r| {
                                                        r.name(new_name);
                                                        r
                                                    }).await {
                                                            Ok(role) => format!("Team name is now {}", new_name),
                                                            Err(err) => format!("Failed to rename team: {:?}", err),
                                                        }
                                                },
                                                _ => "Failed to rename team, could not find team".to_string(),
                                            }
                                        }
                                    },
                                    _ => "Failed to rename team, invalid argument or channel id".to_string()
                                }
                            },
                            "recolor" => {
                                let mut components = Vec::new();
                                for component in &suboption.options {
                                    if let ApplicationCommandInteractionDataOptionValue::Integer(component) = component.resolved.as_ref().expect("Expected integer") {
                                        components.push(component);
                                    }
                                }

                                let new_color = Colour::from_rgb(
                                    *components[0] as u8, 
                                    *components[1] as u8, 
                                    *components[2] as u8
                                );

                                match interaction.channel_id {
                                    Some(channel_id) => {
                                        {
                                            let teams = self.teams.lock().unwrap().get_team(&channel_id);
                                            match teams {
                                                Some(team) => {
                                                    match team.role.edit(ctx.http.clone(), |r| {
                                                        r.colour(new_color.0 as u64);
                                                        r
                                                    }).await {
                                                        Ok(role) => format!("Team color is now ({}, {}, {})", new_color.r(), new_color.g(), new_color.b()),
                                                        Err(err) => format!("Failed to rename team: {:?}", err),
                                                    }
                                                },
                                                _ => "Failed to rename team, could not find team".to_string(),
                                            }
                                        }
                                    },
                                    _ => "Failed to rename team, invalid argument or channel id".to_string()
                                }
                            },
                            "create" => {
                                let host_role = self.host_role.lock().unwrap().unwrap();

                                match &interaction.member {
                                    Some(member) => {
                                        match member.user
                                            .has_role(&ctx.http, interaction.guild_id.expect("Expected guild id"), host_role).await.expect("Expected bool") {
                                            true => {
                                                let channel_arg = suboption.options.get(0).expect("Expected channel id").resolved.as_ref().expect("Expected Channel");
                                                let role_arg = suboption.options.get(1).expect("Expected role id").resolved.as_ref().expect("Expected Role");

                                                match (channel_arg, role_arg) {
                                                    (ApplicationCommandInteractionDataOptionValue::Channel(partial_channel),
                                                    ApplicationCommandInteractionDataOptionValue::Role(role)) => {
                                                        self.create_team(partial_channel.id, role.clone());
                                                        "Created new team".to_string()
                                                    },
                                                    _ => "Failed to create team, unknown channel or role".to_string(),
                                                }
                                            },
                                            false => PERMISSION_DENIED.to_string(),
                                        }
                                    },
                                    None => "No member for interaction".to_string(),
                                }
                            },
                            "score" => {
                                let score_options = suboption.options.get(0).expect("Expected sub-sub option");
                                match score_options.name.as_str().clone() {
                                    "list" => {
                                        let teams = &self.teams.lock().unwrap();
                                        let mut score_list = Vec::new();
                                        for (id, team) in &teams.teams {
                                            score_list.push(format!("{}: {}", team.role.name, team.score));
                                        }

                                        if score_list.len() == 0 {
                                            "No teams created".to_string()
                                        } else {
                                            score_list.join(", ")
                                        }
                                    },
                                    "adjust" => {
                                        let host_role = self.host_role.lock().unwrap().unwrap();
                                        match &interaction.member {
                                            Some(member) => {
                                                match member.user
                                                    .has_role(&ctx.http, interaction.guild_id.expect("Expected guild id"), host_role).await.expect("Expected bool") {
                                                    true => {
                                                        match (*self.teams.lock().unwrap()).teams.get_mut(&interaction.channel_id.expect("Expected channel id")) {
                                                            Some(team) => {
                                                                let adjust_arg = score_options
                                                                    .options
                                                                    .get(0)
                                                                    .expect("Expected adjustment amount")
                                                                    .clone()
                                                                    .resolved
                                                                    .expect("Expected integer");

                                                                match adjust_arg {
                                                                    ApplicationCommandInteractionDataOptionValue::Integer(adjust) => {
                                                                        team.score += adjust;
                                                                        format!("Team score adjusted by {}, score is now {} in total", adjust, team.score)
                                                                    }
                                                                    _ => "Adjustment wrong type, could not adjust".to_string(),
                                                                }
                                                            },
                                                            None => "Missing team, could not adjust".to_string(),
                                                        }
                                                    },
                                                    false => PERMISSION_DENIED.to_string(),
                                                }
                                            },
                                            None => "No member for interaction".to_string(),
                                        }
                                    }
                                    _ => {
                                        "Invalid team->score suboption".to_string()
                                    }
                                }
                            },
                            _ => "Invalid team suboption".to_string(),
                        }
                    },
                    _ => "Invalid command".to_string(),
                };

                if let Err(why) = interaction
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| message.content(content))
                    })
                    .await
                {
                    println!("Cannot respond to slash command: {}", why);
                }
            }
        }
    }

    async fn typing_start(&self, ctx: Context, start: TypingStartEvent) {
        {
            let cache: &Cache = ctx.as_ref();
            if start.user_id == cache.current_user_id().await {
                return
            }
        }

        if let Ok(user) = start.user_id.to_user(ctx.clone()).await {
            println!("mimic: {:?}", user.name);
            start.channel_id.broadcast_typing(ctx).await;
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        for guild in ready.guilds {
            let commands = guild.id().set_application_commands(&ctx.http, |commands| {
                commands
                    .create_application_command(|command| {
                        command.name("ping").description("A ping command")
                    })
                    .create_application_command(|command| {
                        command.name("id").description("Get a user id").create_option(|option| {
                            option
                                .name("id")
                                .description("The user to lookup")
                                .kind(ApplicationCommandOptionType::User)
                                .required(true)
                        })
                    })
                    .create_application_command(|command| {
                        command
                            .name("team")
                            .description("Team options")
                            .create_option(|option| {
                                option
                                    .name("rename")
                                    .description("Rename team.")
                                    .kind(ApplicationCommandOptionType::SubCommand)
                                    .create_sub_option(|option| {
                                        option
                                            .name("name")
                                            .description("New team name")
                                            .kind(ApplicationCommandOptionType::String)
                                            .required(true)
                                    })
                            })
                            .create_option(|option| {
                                option
                                    .name("recolor")
                                    .description("Recolor team")
                                    .kind(ApplicationCommandOptionType::SubCommand)
                                    .create_sub_option(|option| {
                                        option
                                            .name("red")
                                            .description("Red")
                                            .kind(ApplicationCommandOptionType::Integer)
                                            .required(true)
                                    })
                                    .create_sub_option(|option| {
                                        option
                                            .name("green")
                                            .description("Green")
                                            .kind(ApplicationCommandOptionType::Integer)
                                            .required(true)
                                    })
                                    .create_sub_option(|option| {
                                        option
                                            .name("blue")
                                            .description("Blue")
                                            .kind(ApplicationCommandOptionType::Integer)
                                            .required(true)
                                    })
                            })
                            .create_option(|option| {
                                option
                                    .name("create")
                                    .description("Create a team out of an existing channel/role pair.")
                                    .kind(ApplicationCommandOptionType::SubCommand)
                                    .create_sub_option(|option| {
                                        option
                                            .name("channel")
                                            .description("Channel to use for team.")
                                            .kind(ApplicationCommandOptionType::Channel)
                                            .required(true)
                                    })
                                    .create_sub_option(|option| {
                                        option
                                            .name("role")
                                            .description("Role to use for team.")
                                            .kind(ApplicationCommandOptionType::Role)
                                            .required(true)
                                    })
                            })
                            .create_option(|option| {
                                option
                                    .name("score")
                                    .description("Team scores.")
                                    .kind(ApplicationCommandOptionType::SubCommandGroup)
                                    .create_sub_option(|option| {
                                        option
                                            .name("list")
                                            .description("View current score of teams")
                                            .kind(ApplicationCommandOptionType::SubCommand)
                                    })
                                    .create_sub_option(|option| {
                                        option
                                            .name("adjust")
                                            .description("Adjust score for team")
                                            .kind(ApplicationCommandOptionType::SubCommand)
                                            .create_sub_option(|option| {
                                                option
                                                    .name("amount")
                                                    .description("Amount to adjust score")
                                                    .kind(ApplicationCommandOptionType::Integer)
                                                    .required(true)
                                        })
                                    })
                            })
                    })
            })
            .await;

            let fetched_guild = ctx.cache.guild(guild.id()).await;
            if let Some(guild) = fetched_guild {
                    for (role_id, role) in guild.roles {
                        if role.name == "Host" {
                            let mut host_role = self.host_role.lock().unwrap();
                            *host_role = Some(role.id.clone());
                        }
                    }
            }

            println!("I have the following global slash command(s): {:?}", commands);
        }

    }
}

#[tokio::main]
async fn main() {
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // The Application Id is usually the Bot User Id.
    let application_id: u64 =
        env::var("APPLICATION_ID").expect("Expected an application id in the environment").parse().expect("application id is not a valid id");

    // Build our client.
    let mut client = Client::builder(token)
        .event_handler(Handler::new())
        .application_id(application_id)
        .await
        .expect("Error creating client");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}