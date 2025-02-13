use std::{fs::File, path::Path, sync::Arc};

use arrayvec::ArrayVec;
use enum_map::{enum_map, EnumMap};
use ndarray::Array2;
use unnamed_entity::EntityVec;
use winit::event::{ElementState, VirtualKeyCode};

use crate::{
    assets::table::{
        dm::DmFont,
        flippers::{FlipperId, FlipperSide},
        physics::{BumperId, Layer, Material, RollTrigger},
        script::{DmCoord, ScriptBind},
        sound::{JingleBind, SfxBind},
        Assets,
    },
    bcd::Bcd,
    config::{Config, HighScore, Options, Resolution, TableId},
    sound::{controller::TableSequencer, player::Player},
    view::{Action, Route, View},
};

use self::{
    ball::BallState,
    cheat::CheatState,
    dm::DotMatrix,
    lights::Lights,
    party::PartyState,
    physics::{prep_materials, speed_fix, FlipperState, PushState},
    player::PlayerState,
    script::ScriptState,
    scroll::ScrollState,
    show::ShowState,
    speed::SpeedState,
    stones::StonesState,
    tasks::{Task, TaskKind},
};

pub struct Table {
    player: Player,
    sequencer: Arc<TableSequencer>,
    assets: Assets,
    options: Options,
    high_scores: [HighScore; 4],
    hifps: bool,
    scroll: ScrollState,
    lights: Lights,
    push: PushState,
    spring_pos: u8,
    dm: DotMatrix,
    script: ScriptState,
    tasks: Vec<Task>,
    ball: BallState,
    cheat: CheatState,
    flippers: EntityVec<FlipperId, FlipperState>,
    physmaps: EnumMap<Layer, Array2<u8>>,
    materials: [Material; 8],
    kicker_speed_threshold: i16,
    kicker_speed_boost: i16,
    bumper_speed_boost: i16,
    match_timing: [u16; 36],

    in_attract: bool,
    in_game_start: bool,
    in_plunger: bool,
    at_spring: bool,
    in_drain: bool,
    drained: bool,
    got_top_score: bool,
    party_on: bool,
    special_plunger_event: bool,
    match_digit: Option<u8>,
    ball_scored_points: bool,
    tilted: bool,
    tilt_counter: u16,
    silence_effect: bool,
    timer_stop: bool,
    block_drain: bool,
    got_high_score: bool,
    flush_high_scores: bool,
    name_buf: ArrayVec<u8, 3>,

    in_mode: bool,
    in_mode_hit: bool,
    in_mode_ramp: bool,
    pending_mode: bool,
    pending_mode_hit: bool,
    pending_mode_ramp: bool,
    mode_timeout_frames: u8,
    mode_timeout_secs: u8,

    kbd_state: KbdState,
    flipper_state: EnumMap<FlipperSide, bool>,
    flipper_pressed: bool,
    flippers_enabled: bool,
    space_state: bool,
    space_pressed: bool,
    spring_down_state: bool,
    spring_released: bool,
    start_keys_active: bool,
    start_key: Option<u8>,

    quitting: bool,
    fade: u16,

    cur_player: u8,
    total_players: u8,
    cur_ball: u8,
    total_balls: u8,
    extra_balls: u8,
    bonus_mult_early: u8,
    bonus_mult_late: u8,
    players: Vec<PlayerState>,

    score_main: Bcd,
    score_bonus: Bcd,
    score_jackpot: Bcd,
    score_mode_hit: Bcd,
    score_mode_ramp: Bcd,
    score_raising_millions: Bcd,
    num_cyclone: u16,
    num_cyclone_target: u16,
    bcd_num_cyclone: Bcd,
    score_cyclone_bonus: Bcd,
    hold_bonus: bool,

    hit_pos: Option<(i16, i16)>,
    hit_bumper: Option<BumperId>,
    roll_trigger: Option<RollTrigger>,
    prev_roll_trigger: Option<RollTrigger>,

    party: PartyState,
    speed: SpeedState,
    show: ShowState,
    stones: StonesState,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KbdState {
    Main,
    ConfirmQuit,
    Paused,
    PausedConfirmQuit,
    GetName,
}

mod ball;
mod cheat;
mod dm;
mod flippers;
mod game;
mod lights;
mod mode;
mod party;
mod physics;
mod player;
mod script;
mod scroll;
mod show;
mod sound;
mod speed;
mod stones;
mod tasks;
mod triggers;

impl Table {
    pub fn new(data: &Path, config: Config, table: TableId) -> Table {
        let options = config.options;
        let high_scores = config.high_scores[table];
        let (prg, module) = match table {
            TableId::Table1 => ("TABLE1.PRG", "TABLE1.MOD"),
            TableId::Table2 => ("TABLE2.PRG", "TABLE2.MOD"),
            TableId::Table3 => ("TABLE3.PRG", "TABLE3.MOD"),
            TableId::Table4 => ("TABLE4.PRG", "TABLE4.MOD"),
        };
        let mut f = File::open(data.join(module)).unwrap();
        let assets = Assets::load(data.join(prg), table).unwrap();
        let module = crate::sound::loader::load(&mut f).unwrap();
        let sequencer = Arc::new(TableSequencer::new(
            assets.jingle_binds[JingleBind::Attract].unwrap().position,
            assets.position_jingle_start,
            assets.jingle_binds[JingleBind::Silence].unwrap().position,
            options.no_music,
        ));
        let player = crate::sound::player::play(module, Some(sequencer.clone()));

        let hifps = false;
        let scroll = ScrollState::new(&options);
        let lights = Lights::new(&assets);
        let flippers = assets
            .flippers
            .map_values(|flipper| FlipperState::new(flipper, hifps));
        let physmaps = assets.physmaps.clone();
        let materials = prep_materials(hifps);

        let mut res = Table {
            player,
            sequencer,
            assets,
            options,
            high_scores,
            hifps,
            scroll,
            lights,
            push: PushState::new(hifps),
            spring_pos: 0,
            dm: DotMatrix::new(),
            script: ScriptState::new(),
            tasks: vec![],
            ball: BallState::new(hifps),
            cheat: CheatState::new(),
            flippers,
            physmaps,
            materials,
            kicker_speed_threshold: speed_fix(300, hifps),
            kicker_speed_boost: speed_fix(2000, hifps),
            bumper_speed_boost: speed_fix(7000, hifps),
            match_timing: if hifps {
                [
                    22, 28, 25, 25, 22, 19, 18, 15, 13, 11, 9, 9, 8, 8, 7, 7, 6, 6, 6, 6, 6, 5, 5,
                    5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 3, 3,
                ]
            } else {
                [
                    24, 23, 21, 21, 18, 16, 15, 13, 11, 9, 8, 7, 7, 6, 6, 6, 5, 5, 5, 5, 5, 4, 4,
                    4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3,
                ]
            },

            in_attract: true,
            in_plunger: true,
            at_spring: false,
            in_drain: false,
            drained: false,
            got_top_score: false,
            got_high_score: false,
            flush_high_scores: false,
            in_game_start: true,
            party_on: false,
            special_plunger_event: false,
            match_digit: None,
            ball_scored_points: false,
            tilted: false,
            tilt_counter: 0,
            silence_effect: false,
            timer_stop: false,
            block_drain: false,
            name_buf: ArrayVec::new(),

            in_mode: false,
            in_mode_hit: false,
            in_mode_ramp: false,
            pending_mode: false,
            pending_mode_hit: false,
            pending_mode_ramp: false,
            mode_timeout_secs: 0,
            mode_timeout_frames: 0,

            kbd_state: KbdState::Main,
            flipper_state: enum_map! { _ => false},
            flipper_pressed: false,
            flippers_enabled: false,
            space_state: false,
            space_pressed: false,
            spring_down_state: false,
            spring_released: false,
            start_keys_active: true,
            start_key: None,
            quitting: false,
            fade: 0x100,

            cur_player: 1,
            total_players: 1,
            cur_ball: 1,
            total_balls: config.options.balls,
            extra_balls: 0,
            bonus_mult_early: 1,
            bonus_mult_late: 1,
            players: vec![],

            score_main: Bcd::ZERO,
            score_bonus: Bcd::ZERO,
            score_jackpot: Bcd::ZERO,
            score_mode_hit: Bcd::ZERO,
            score_mode_ramp: Bcd::ZERO,
            score_raising_millions: Bcd::ZERO,
            num_cyclone: 0,
            num_cyclone_target: 0,
            bcd_num_cyclone: Bcd::ZERO,
            score_cyclone_bonus: Bcd::ZERO,
            hold_bonus: false,

            hit_pos: None,
            hit_bumper: None,
            roll_trigger: None,
            prev_roll_trigger: None,

            party: PartyState::new(),
            speed: SpeedState::new(),
            show: ShowState::new(hifps),
            stones: StonesState::new(),
        };
        res.ball.set_pos((280, 525));
        res.start_script(ScriptBind::Init);
        res.flippers_physmap_update();
        res
    }

    pub fn pause(&mut self) {
        self.dm.save();
        self.dm.clear();
        self.dm.set_state(true);
        self.dm_puts(DmFont::H13, DmCoord { x: 36, y: 1 }, b"GAME PAUSED");
        self.kbd_state = KbdState::Paused;
        self.player.pause();
    }

    pub fn unpause(&mut self) {
        self.dm.restore();
        self.kbd_state = KbdState::Main;
        self.player.unpause();
    }

    pub fn toggle_music(&mut self) {
        if self.options.no_music {
            self.options.no_music = false;
            let bind = if self.in_plunger {
                JingleBind::Plunger
            } else {
                JingleBind::Main
            };
            let jingle = self.assets.jingle_binds[bind].unwrap();
            self.sequencer.set_music(jingle.position);
            self.sequencer.force_end_loop();
        } else {
            self.options.no_music = true;
            self.play_jingle_bind_force(JingleBind::Silence);
        }
        self.sequencer.set_no_music(self.options.no_music);
    }
}

impl View for Table {
    fn get_resolution(&self) -> (u32, u32) {
        (
            320,
            match self.options.resolution {
                Resolution::Normal => 240,
                Resolution::High => 350,
                Resolution::Full => 576 + 33,
            },
        )
    }

    fn get_fps(&self) -> u32 {
        60
    }

    fn run_frame(&mut self) -> Action {
        if matches!(
            self.kbd_state,
            KbdState::Paused | KbdState::PausedConfirmQuit
        ) {
            Action::None
        } else if self.quitting {
            self.fade -= 2;
            self.player.set_master_volume(self.fade.into());
            if self.fade == 0 {
                Action::Navigate(Route::Intro(Some(self.assets.table)))
            } else {
                Action::None
            }
        } else {
            if self.in_attract {
                self.scroll.attract_frame();
                self.lights.attract_frame(&self.assets);
                self.dm.blink_frame();
                if let Some(players) = self.start_key {
                    self.start_key = None;
                    self.total_players = players;
                    self.players = vec![PlayerState::new(self.assets.table); players as usize];
                    self.start_script(ScriptBind::GameStart);
                    self.play_sfx_bind(SfxBind::GameStart);
                    self.in_attract = false;
                    self.init_game();
                    let jingle = self.assets.jingle_binds[JingleBind::GameStart].unwrap();
                    let plunger = self.assets.jingle_binds[if self.options.no_music {
                        JingleBind::Silence
                    } else {
                        JingleBind::Plunger
                    }]
                    .unwrap();
                    self.sequencer
                        .play_jingle(jingle, true, Some(plunger.position));
                    self.issue_ball();
                    self.add_task(TaskKind::SetStartKeysActive);
                }
            } else {
                self.scroll.update(self.ball.pos().1);
                if let Some(players) = self.start_key {
                    self.start_key = None;
                    self.total_players = players;
                    self.players = vec![PlayerState::new(self.assets.table); players as usize];
                    self.start_script(ScriptBind::GameStartPlayers);
                    self.play_sfx_bind(SfxBind::GameStart);
                    self.add_task(TaskKind::SetStartKeysActive);
                }
                if !self.cheat.slowdown {
                    self.physics_frame();
                }
                self.physics_frame();
                self.physics_frame();
                self.physics_frame();
                if self.tilt_counter != 0 {
                    self.tilt_counter -= 1;
                }
                self.score_bumper();
                self.ball_gravity();
                self.check_transitions();
                if self.drained && !self.in_drain {
                    self.ball.teleport_freeze(Layer::Ground, (280, 525));
                    self.flippers_enabled = false;
                    self.in_mode = false;
                    self.in_mode_hit = false;
                    self.in_mode_ramp = false;
                    if !self.block_drain {
                        self.in_drain = true;
                        match self.assets.table {
                            TableId::Table1 => self.party_drained(),
                            TableId::Table2 => self.speed_drained(),
                            TableId::Table3 => self.show_drained(),
                            TableId::Table4 => self.stones_drained(),
                        }
                    }
                }
                match self.assets.table {
                    TableId::Table1 => self.party_frame(),
                    TableId::Table2 => self.speed_frame(),
                    TableId::Table3 => self.show_frame(),
                    TableId::Table4 => self.stones_frame(),
                };
                self.do_roll_triggers();
                self.do_hit_triggers();
                if self.flipper_pressed {
                    self.flipper_pressed = false;
                    match self.assets.table {
                        TableId::Table1 => self.party_flipper_pressed(),
                        TableId::Table2 => self.speed_flipper_pressed(),
                        TableId::Table3 => self.show_flipper_pressed(),
                        TableId::Table4 => self.stones_flipper_pressed(),
                    }
                }
                if self.space_pressed {
                    self.space_pressed = false;
                    if !self.cheat.no_tilt && !self.in_plunger && !self.drained && !self.tilted {
                        self.tilt_counter += 60;
                        if self.tilt_counter > 120 {
                            self.tilted = true;
                            self.flippers_enabled = false;
                            self.play_jingle_bind_silence(JingleBind::Tilt);
                            self.start_script(ScriptBind::Tilt);
                            self.lights.tilt();
                            self.party.secret_drop_release = true;
                        } else if self.tilt_counter > 60 {
                            self.play_jingle_bind(JingleBind::WarnTilt);
                        }
                    }
                }
                self.dm.blink_frame();
                self.tasks_frame();
                self.lights.blink_frame();
                if self.spring_released && self.spring_pos != 0 {
                    self.spring_release();
                    self.spring_released = false;
                } else if self.spring_down_state && self.spring_pos < 0x20 {
                    self.spring_pos += 1;
                }
            }
            self.script_frame();
            if self.flush_high_scores {
                self.flush_high_scores = false;
                Action::SaveHighScores(self.assets.table, self.high_scores)
            } else {
                Action::None
            }
        }
    }

    fn handle_key(&mut self, key: VirtualKeyCode, state: ElementState) {
        if matches!(
            key,
            VirtualKeyCode::LShift | VirtualKeyCode::LControl | VirtualKeyCode::LAlt
        ) {
            if state == ElementState::Pressed
                && self.flippers_enabled
                && !self.flipper_state[FlipperSide::Left]
            {
                self.flipper_pressed = true;
                self.play_sfx_bind(SfxBind::FlipperPress);
            }
            self.flipper_state[FlipperSide::Left] = state == ElementState::Pressed;
        }
        if matches!(
            key,
            VirtualKeyCode::RShift | VirtualKeyCode::RControl | VirtualKeyCode::RAlt
        ) {
            if state == ElementState::Pressed
                && self.flippers_enabled
                && !self.flipper_state[FlipperSide::Right]
            {
                self.flipper_pressed = true;
                self.play_sfx_bind(SfxBind::FlipperPress);
            }
            self.flipper_state[FlipperSide::Right] = state == ElementState::Pressed;
        }

        if key == VirtualKeyCode::Space {
            if state == ElementState::Pressed && !self.space_state {
                self.space_pressed = true;
            }
            self.space_state = state == ElementState::Pressed;
        }

        if key == VirtualKeyCode::Down {
            self.spring_down_state = state == ElementState::Pressed;
            if state == ElementState::Released {
                self.spring_released = true;
            }
        }

        if state != ElementState::Pressed {
            return;
        }

        let chr = match key {
            VirtualKeyCode::A => Some(b'A'),
            VirtualKeyCode::B => Some(b'B'),
            VirtualKeyCode::C => Some(b'C'),
            VirtualKeyCode::D => Some(b'D'),
            VirtualKeyCode::E => Some(b'E'),
            VirtualKeyCode::F => Some(b'F'),
            VirtualKeyCode::G => Some(b'G'),
            VirtualKeyCode::H => Some(b'H'),
            VirtualKeyCode::I => Some(b'I'),
            VirtualKeyCode::J => Some(b'J'),
            VirtualKeyCode::K => Some(b'K'),
            VirtualKeyCode::L => Some(b'L'),
            VirtualKeyCode::M => Some(b'M'),
            VirtualKeyCode::N => Some(b'N'),
            VirtualKeyCode::O => Some(b'O'),
            VirtualKeyCode::P => Some(b'P'),
            VirtualKeyCode::Q => Some(b'Q'),
            VirtualKeyCode::R => Some(b'R'),
            VirtualKeyCode::S => Some(b'S'),
            VirtualKeyCode::T => Some(b'T'),
            VirtualKeyCode::U => Some(b'U'),
            VirtualKeyCode::V => Some(b'V'),
            VirtualKeyCode::W => Some(b'W'),
            VirtualKeyCode::X => Some(b'X'),
            VirtualKeyCode::Y => Some(b'Y'),
            VirtualKeyCode::Z => Some(b'Z'),
            VirtualKeyCode::Space => Some(b' '),
            _ => None,
        };

        match self.kbd_state {
            KbdState::Main => {
                match key {
                    VirtualKeyCode::F9 => self.scroll.set_speed(9),
                    VirtualKeyCode::F10 => self.scroll.set_speed(11),
                    VirtualKeyCode::F11 => self.scroll.set_speed(20),
                    VirtualKeyCode::F12 => self.scroll.set_speed(40),
                    _ => (),
                }

                if self.start_keys_active && (self.in_attract || self.at_spring) {
                    match key {
                        VirtualKeyCode::F1 => self.start_key = Some(1),
                        VirtualKeyCode::F2 => self.start_key = Some(2),
                        VirtualKeyCode::F3 => self.start_key = Some(3),
                        VirtualKeyCode::F4 => self.start_key = Some(4),
                        VirtualKeyCode::F5 => self.start_key = Some(5),
                        VirtualKeyCode::F6 => self.start_key = Some(6),
                        VirtualKeyCode::F7 => self.start_key = Some(7),
                        VirtualKeyCode::F8 => self.start_key = Some(8),
                        VirtualKeyCode::Return => {
                            if self.in_attract {
                                self.start_key = Some(1);
                            } else if self.total_players < 8 {
                                self.start_key = Some(self.total_players + 1);
                            }
                        }
                        _ => (),
                    }
                    if self.start_key.is_some() {
                        self.start_keys_active = false;
                    }
                }

                if self.in_attract {
                    if let Some(chr) = chr {
                        self.handle_cheat(chr);
                    }
                    if key == VirtualKeyCode::Escape {
                        self.kbd_state = KbdState::ConfirmQuit;
                        self.start_script(ScriptBind::ConfirmQuit);
                    }
                } else if !self.in_drain {
                    match key {
                        VirtualKeyCode::Escape if self.at_spring => self.abort_game(),
                        VirtualKeyCode::M => self.toggle_music(),
                        VirtualKeyCode::P => self.pause(),
                        // VirtualKeyCode::W => self.ball.speed = (0, -1000),
                        // VirtualKeyCode::S => self.ball.speed = (0, 1000),
                        // VirtualKeyCode::A => self.ball.speed = (-1000, 0),
                        // VirtualKeyCode::D => self.ball.speed = (1000, 0),
                        _ => (),
                    }
                }
            }
            KbdState::ConfirmQuit => {
                if state != ElementState::Pressed {
                    return;
                }
                match key {
                    VirtualKeyCode::Y => {
                        self.quitting = true;
                        self.kbd_state = KbdState::Main;
                    }
                    VirtualKeyCode::N => self.kbd_state = KbdState::Main,
                    _ => (),
                }
            }
            KbdState::Paused => {
                if state != ElementState::Pressed {
                    return;
                }
                if key == VirtualKeyCode::Escape {
                    self.dm.clear();
                    self.dm_puts(DmFont::H13, DmCoord { x: 0, y: 1 }, b"REALLY QUIT (Y OR N)");
                    self.kbd_state = KbdState::PausedConfirmQuit;
                } else {
                    self.unpause();
                }
            }
            KbdState::PausedConfirmQuit => {
                if state != ElementState::Pressed {
                    return;
                }
                if key == VirtualKeyCode::Y {
                    self.dm.restore();
                    self.quitting = true;
                    self.kbd_state = KbdState::Main;
                } else {
                    self.unpause();
                }
            }
            KbdState::GetName => {
                if let Some(chr) = chr {
                    let _ = self.name_buf.try_push(chr);
                }
            }
        }
    }

    fn render(&self, data: &mut [u8], pal: &mut [(u8, u8, u8)]) {
        pal.copy_from_slice(&self.assets.main_board.cmap);
        for (lid, light) in &self.assets.lights {
            if self.lights.is_lit(lid) {
                for (i, color) in light.colors.iter().enumerate() {
                    pal[light.base_index as usize + i] = *color;
                }
            } else {
                for (i, color) in light.colors.iter().enumerate() {
                    pal[light.base_index as usize + i] = (color.0 / 2, color.1 / 2, color.2 / 2);
                }
            }
        }
        pal[self.assets.dm_palette.index_on as usize] = if self.dm.state() {
            self.assets.dm_palette.color_on
        } else {
            self.assets.dm_palette.color_off
        };
        let height = match self.options.resolution {
            Resolution::Normal => 240 - 33,
            Resolution::High => 350 - 33,
            Resolution::Full => 576,
        };
        let spring_pos = self.spring_pos as usize / 2;
        let (bx, mut by) = self.ball.pos();
        if !self.ball.frozen {
            by += self.push.offset();
        }
        for y in 0..height {
            let sy = y + self.scroll.pos() as usize + self.push.offset() as usize;
            if sy >= 576 {
                for x in 0..320 {
                    data[y * 320 + x] = 0;
                }
            } else {
                for x in 0..320 {
                    data[y * 320 + x] = self.assets.main_board.data[(x, sy)];
                }
            }
            if (556..556 + 17).contains(&sy) {
                let spring_y = sy - 553;
                if spring_y >= spring_pos {
                    let spring_y = spring_y - spring_pos;
                    for spring_x in 0..10 {
                        data[y * 320 + spring_x + 304] =
                            self.assets.spring.data[(spring_x, spring_y)];
                    }
                }
            }
            for (fid, flipper) in &self.assets.flippers {
                let state = &self.flippers[fid];
                let gfx = &flipper.gfx[state.quantum as usize];
                if sy >= (flipper.rect_pos.1 as usize)
                    && (sy - (flipper.rect_pos.1 as usize)) < gfx.dim().1
                {
                    let fy = sy - (flipper.rect_pos.1 as usize);
                    for fx in 0..gfx.dim().0 {
                        data[y * 320 + fx + (flipper.rect_pos.0 as usize)] = gfx[(fx, fy)];
                    }
                }
            }
            if (by..by + 15).contains(&(sy as i16)) {
                let ball_y = sy as i16 - by;
                for ball_x in 0..15 {
                    let pix = self.assets.ball.data[(ball_x as usize, ball_y as usize)];
                    if pix == 0 {
                        continue;
                    }
                    let x = ball_x + bx;
                    if !(0..320).contains(&x) {
                        continue;
                    }
                    if sy < 576 && self.assets.occmaps[self.ball.layer][(x as usize, sy)] != 0 {
                        continue;
                    }
                    data[y * 320 + x as usize] = pix;
                }
            }
        }
        for y in 0..16 {
            let dy = 2 + 2 * y + height;
            for x in 0..160 {
                let pix = if self.dm.pixels[y][x] {
                    self.assets.dm_palette.index_on
                } else {
                    self.assets.dm_palette.index_off
                };
                data[dy * 320 + x * 2] = pix;
            }
        }

        if self.options.mono {
            for color in &mut pal[..] {
                let mono = ((color.0 as u16 + color.1 as u16 + color.2 as u16) / 3) as u8;
                *color = (mono, mono, mono);
            }
        }

        if self.fade != 0x100 {
            for color in pal {
                color.0 = (((color.0 as u16) * self.fade) >> 8) as u8;
                color.1 = (((color.1 as u16) * self.fade) >> 8) as u8;
                color.2 = (((color.2 as u16) * self.fade) >> 8) as u8;
            }
        }
    }
}
