use freven_api::{ClientNameplateDrawCmd, ClientTickApi};

const ENABLED: bool = true;
const WORLD_Y_OFFSET_M: f32 = 1.4;
const MAX_DISTANCE_M: f32 = 48.0;
const MAX_VISIBLE: usize = 64;

pub fn start_client(api: &mut freven_api::ClientApi<'_>) {
    api.nameplates.clear_nameplates();
}

pub fn tick_client(tick: &mut ClientTickApi<'_>) {
    let api = &mut tick.client;
    api.nameplates.clear_nameplates();
    if !ENABLED {
        return;
    }

    let mut players = Vec::new();
    api.players.list_players(&mut players);
    if players.is_empty() {
        return;
    }

    let local_pos = players.iter().find(|p| p.is_local).map(|p| p.world_pos_m);
    let mut emitted = 0_usize;

    for player in players {
        if player.is_local {
            continue;
        }
        if emitted >= MAX_VISIBLE {
            break;
        }

        if let Some(origin) = local_pos
            && distance_squared(player.world_pos_m, origin) > MAX_DISTANCE_M * MAX_DISTANCE_M
        {
            continue;
        }

        let world = (
            player.world_pos_m.0,
            player.world_pos_m.1 + WORLD_Y_OFFSET_M,
            player.world_pos_m.2,
        );
        let Some(screen) = api.players.world_to_screen(world) else {
            continue;
        };

        let text = api
            .players
            .display_name_for(player.player_id)
            .unwrap_or_else(|| format!("player#{}", player.player_id));

        api.nameplates.push_nameplate(ClientNameplateDrawCmd {
            text,
            screen_pos_px: screen,
            rgba: (245, 245, 245, 220),
        });
        emitted = emitted.saturating_add(1);
    }
}

fn distance_squared(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    let dz = a.2 - b.2;
    dx * dx + dy * dy + dz * dz
}
