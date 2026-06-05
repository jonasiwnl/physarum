extern crate image;
extern crate opengl_graphics;
extern crate piston_window;
extern crate rand;
extern crate rayon;

use image::{ImageBuffer, Rgba};
use opengl_graphics::OpenGL;
use piston_window::*;
use rand::distributions::Uniform;
use rand::*;
use rayon::prelude::*;
use serde::Deserialize;
use std::f64::consts::PI;
use std::fs;
use std::path::Path;

const CONFIG_PATH: &str = "src/.config";

#[derive(Copy, Clone, Deserialize)]
#[serde(default)]
struct Config {
    width: f64,
    height: f64,
    agents: usize,
    agent_color: [u8; 4],
    sensor_offset_angle: f64,
    sensor_offset_dst: f64,
    sensor_r: isize,
    turn_strength: f64,
    spawn_type: SpawnType,
    circle_angle: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 500.,
            height: 500.,
            agents: 20_000,
            agent_color: [150, 0, 200, 50],
            sensor_offset_angle: PI / 8.,
            sensor_offset_dst: 15.,
            sensor_r: 2,
            turn_strength: PI / 8.,
            spawn_type: SpawnType::Circle,
            circle_angle: PI * 2.,
        }
    }
}

impl Config {
    fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();

        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(format!("failed to read {}: {}", path.display(), err)),
        };

        toml::from_str(&contents)
            .map_err(|err| format!("failed to parse {}: {}", path.display(), err))
    }
}

#[derive(Copy, Clone)]
struct Agent {
    x: f64,
    y: f64,
    ang: f64,
}

impl Agent {
    fn new() -> Self {
        Agent {
            x: 0.,
            y: 0.,
            ang: 0.,
        }
    }
    fn update(&mut self, config: &Config) {
        let mut new_x = self.x + self.ang.cos();
        let mut new_y = self.y + self.ang.sin();
        let mut rng = thread_rng();
        if new_x > config.width - 1. || new_x < 0. {
            new_x = if if new_x < 0. { 0. } else { new_x } > config.width - 1. {
                config.width - 1.
            } else {
                new_x
            };
            self.ang = rng.gen_range(0.0..2. * PI);
        }
        if new_y > config.height - 1. || new_y < 0. {
            new_y = if if new_y < 0. { 0. } else { new_y } > config.height - 1. {
                config.height - 1.
            } else {
                new_y
            };
            self.ang = rng.gen_range(0.0..2. * PI);
        }
        self.x = new_x;
        self.y = new_y;
    }
    fn check(&mut self, img: &ImageBuffer<Rgba<u8>, Vec<u8>>, config: &Config) {
        let weight_forward = self.sense(0., img, config);
        let weight_right = self.sense(config.sensor_offset_angle, img, config);
        let weight_left = self.sense(-config.sensor_offset_angle, img, config);
        let rng = thread_rng().gen_range(0.0..1.01);
        if weight_right < weight_forward && weight_forward > weight_left {
        } else if weight_right == weight_left {
            self.ang += (rng - 0.5) * 0.2 * config.turn_strength;
        } else if weight_right > weight_left {
            self.ang += rng * config.turn_strength;
        } else {
            self.ang -= rng * config.turn_strength;
        }
    }
    fn sense(
        &mut self,
        offset_angle: f64,
        img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
        config: &Config,
    ) -> f64 {
        let angle = self.ang + offset_angle;
        let center_x = (self.x + angle.cos() * config.sensor_offset_dst) as isize;
        let center_y = (self.y + angle.sin() * config.sensor_offset_dst) as isize;
        let mut sum: f64 = 0.;
        for x in center_x - config.sensor_r..=center_x + config.sensor_r {
            for y in center_y - config.sensor_r..=center_y + config.sensor_r {
                if x >= 0 && x < config.width as isize && y >= 0 && y < config.height as isize {
                    let pixel = img.get_pixel(x as u32, y as u32);
                    sum += pixel[2] as f64 / 255.;
                    sum += pixel[1] as f64 / 255.;
                }
            }
        }
        sum
    }
}

struct Simulation {
    agents: Vec<Agent>,
    config: Config,
}

impl Simulation {
    fn new(config: Config) -> Self {
        let uniform: Uniform<f64> = Uniform::<f64>::new(0., 1.);
        let mut rng = thread_rng();
        let mut agents = vec![Agent::new(); config.agents];
        match config.spawn_type {
            SpawnType::Random => {
                for agent in agents.iter_mut() {
                    agent.x = rng.sample(uniform) * config.width;
                    agent.y = rng.sample(uniform) * config.height;
                    agent.ang = rng.sample(uniform) * 2. * PI;
                }
            }
            SpawnType::Circle => {
                for agent in agents.iter_mut() {
                    let angle = rng.sample(uniform) * 2. * PI;
                    let rad = rng.sample(uniform) * (config.height / 2. - 1.);
                    agent.x = config.width / 2. + angle.cos() * rad;
                    agent.y = config.height / 2. + angle.sin() * rad;
                    agent.ang = angle * config.circle_angle;
                }
            }
            SpawnType::Waterfall => {
                for agent in agents.iter_mut() {
                    agent.x = rng.sample(uniform) * config.width;
                    agent.y = rng.sample(uniform) * config.height;
                    agent.ang = PI / 2.;
                }
            }
            SpawnType::Point => {
                for agent in agents.iter_mut() {
                    agent.x = config.width / 2.;
                    agent.y = config.height / 2.;
                    agent.ang = rng.sample(uniform) * 2. * PI;
                }
            }
            _ => {}
        }
        Simulation { agents, config }
    }
}

#[derive(Copy, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SpawnType {
    Random,
    Circle,
    Waterfall,
    Point,
    Lines,
}

fn reduce_pixel(value: u8) -> u8 {
    if value > 3 {
        return value - 3;
    } else if value > 0 {
        return 0;
    }
    0
}

fn main() -> () {
    let config = Config::load(CONFIG_PATH).unwrap_or_else(|err| {
        eprintln!("Config error: {}", err);
        std::process::exit(1);
    });
    let mut sim = Simulation::new(config);

    let opengl = OpenGL::V3_2;
    let mut window: PistonWindow =
        WindowSettings::new("physarum", [sim.config.width, sim.config.height])
            .graphics_api(opengl)
            .exit_on_esc(true)
            .build()
            .unwrap();

    let mut img = ImageBuffer::new(sim.config.width as u32, sim.config.height as u32);

    let mut events = Events::new(EventSettings::new());
    events.set_max_fps(60);
    while let Some(en) = events.next(&mut window) {
        let texture: G2dTexture = Texture::from_image(
            &mut window.create_texture_context(),
            &img,
            &TextureSettings::new(),
        )
        .unwrap();
        window.draw_2d(&en, |c, g, _d| {
            sim.agents.par_iter_mut().for_each(|agent| {
                agent.check(&img, &sim.config);
                agent.update(&sim.config);
            });
            for agent in &sim.agents {
                img.put_pixel(agent.x as u32, agent.y as u32, Rgba(sim.config.agent_color));
            }
            image(&texture, c.transform, g);
            for pixel in img.pixels_mut() {
                pixel[0] = reduce_pixel(pixel[0]); // for different colors
                pixel[1] = reduce_pixel(pixel[1]);
                pixel[2] = reduce_pixel(pixel[2]);
            }
        });
    }
}
