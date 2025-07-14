use async_std::task::{self, JoinHandle};
use futures::{future::join_all, lock::Mutex};
use num::complex::Complex;
use speedy2d::{
    color::Color,
    dimen::Vector2,
    window::{WindowHandler, WindowHelper},
    Graphics2D, Window,
};
use std::{
    collections::VecDeque,
    env,
    future::{ready, IntoFuture, Ready},
    ops::Add,
    sync::{
        atomic::{AtomicU8, AtomicUsize},
        Arc,
    },
    thread::{self, sleep},
    time::{self, Duration, Instant},
};

const WINDOW_WIDTH: u32 = 2048;
const WINDOW_HEIGHT: u32 = 2048;
const MAX_DEPTH: u32 = 10000;
const THREAD_COUNT: usize = 32;

#[derive(Copy, Clone)]
struct Zoom {
    zoom_factor: f64,
    factor_x: f64,
    factor_y: f64,
    term_x: f64,
    term_y: f64,
}

impl Zoom {
    fn new(zoom_factor: f64, center_x: f64, center_y: f64) -> Zoom {
        let factor_x = 4.0 / (zoom_factor * WINDOW_WIDTH as f64);
        let factor_y = 4.0 / (zoom_factor * WINDOW_HEIGHT as f64);
        let term_x = center_x - 2.0 / zoom_factor;
        let term_y = center_y - 2.0 / zoom_factor;
        return Zoom {
            zoom_factor,
            factor_x,
            factor_y,
            term_x,
            term_y,
        };
    }
}

#[derive(Clone)]
struct Bitmap {
    pixels: Vec<u8>,
    size: (u32, u32),
    location: (u32, u32),
}

impl IntoFuture for Bitmap {
    type Output = Bitmap;
    type IntoFuture = Ready<Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        ready(self)
    }
}

struct WorkItem {
    br_x: u32,
    br_y: u32,
    tl_x: u32,
    tl_y: u32,
    must_split: bool,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let timings = args.iter().any(|a| a == "timing");
    let window =
        Window::new_centered("Title", (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)).unwrap();
    let zoom_status = Zoom::new(1.0, 0.0, 0.0);
    let mouse_position: Vector2<f64> = Vector2::new(0.0, 0.0);
    let bitmaps: Vec<Bitmap> = vec![];

    window.run_loop(MyWindowHandler {
        zoom_status,
        mouse_position,
        bitmaps,
        should_clear: false,
        timings: timings,
    });
}

fn test(window_handler: &mut MyWindowHandler, helper: &WindowHelper) {
    let total = Instant::now();

    POINTS.iter().for_each(|p| {
        let position: Vector2<f64> = pixel_to_coordinate(p.x, p.y, &mut window_handler.zoom_status);
        let z = window_handler.zoom_status;
        window_handler.zoom_status = zoom_to(true, position, &z);

        futures::executor::block_on(async {
            let split_spawn_time = Instant::now();
            window_handler.bitmaps = split_and_spawn(z).await;
            helper.request_redraw();

            let t = split_spawn_time.elapsed().as_millis() as f32 / 1000.0;
            println!("{}x t: {}", z.zoom_factor, t);
        });
    });
    let t = total.elapsed().as_millis() as f32 / 1000.0;
    println!("total: {}", t);
}

fn calculate_color(c: Complex<f64>) -> Color {
    let mut x2 = 0.0;
    let mut y2 = 0.0;
    let mut w = 0.0;
    let mut depth = 0;

    while x2 + y2 < 4.0 && depth < MAX_DEPTH {
        let x = x2 - y2 + c.re;
        let y = w - x2 - y2 + c.im;
        x2 = x * x;
        y2 = y * y;
        w = x + y;
        w = w * w;
        depth += 1;
    }
    if depth == MAX_DEPTH {
        Color::BLACK
    } else {
        Color::from_hex_rgb(depth)
    }
}

fn pixel_to_coordinate(x: f64, y: f64, zoom: &Zoom) -> Vector2<f64> {
    let x_pixel: f64 = x * zoom.factor_x + zoom.term_x;
    let y_pixel: f64 = y * zoom.factor_y + zoom.term_y;
    Vector2::new(x_pixel, y_pixel)
}

fn zoom_to(in_out: bool, location: Vector2<f64>, zoom: &Zoom) -> Zoom {
    let zoom_factor = if in_out {
        zoom.zoom_factor * 2.0
    } else {
        zoom.zoom_factor / 2.0
    };
    let center_x = location.x;
    let center_y = location.y;
    Zoom::new(zoom_factor, center_x, center_y)
}

fn get_pixel_color(x: u32, y: u32, zoom: &Zoom) -> Color {
    let coordinates = pixel_to_coordinate(x as f64, y as f64, zoom);
    let c = Complex::new(coordinates.x, coordinates.y);
    return calculate_color(c);
}

fn set_pixel_color(x: u32, y: u32, color: Color, bitmap: &mut Bitmap) {
    let pixel = (x - bitmap.location.0 + bitmap.size.0 * (y - bitmap.location.1)) * 4;
    // println!(
    //     "{} - {} + {} * ({} - {})",
    //     x, bitmap.location.0, bitmap.size.0, y, bitmap.location.1
    // );
    // println!("{}", pixel);
    bitmap.pixels[pixel as usize] = (color.r() * 255.0) as u8;
    bitmap.pixels[pixel as usize + 1] = (color.g() * 255.0) as u8;
    bitmap.pixels[pixel as usize + 2] = (color.b() * 255.0) as u8;
    bitmap.pixels[pixel as usize + 3] = 255 as u8;
}

fn calculate_rectangle(
    WorkItem {
        br_x,
        br_y,
        tl_x,
        tl_y,
        ..
    }: WorkItem,
    zoom: &Zoom,
    bitmap: &mut Bitmap,
) {
    for x in tl_x..=br_x {
        for y in tl_y..=br_y {
            let color = get_pixel_color(x, y, zoom);
            set_pixel_color(x, y, color, bitmap);
        }
    }
}

async fn split_and_spawn(zoom: Zoom) -> Vec<Bitmap> {
    let mut threads: Vec<JoinHandle<_>> = Vec::with_capacity(THREAD_COUNT);
    let bitmaps: Arc<Mutex<Vec<Bitmap>>> = Arc::new(Mutex::new(vec![]));
    // let busy_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let busy_count: Arc<Mutex<u8>> = Arc::new(Mutex::new(0));
    let deque: Arc<Mutex<VecDeque<WorkItem>>> = Arc::new(Mutex::new(VecDeque::new()));
    deque.lock().await.push_back(WorkItem {
        br_x: WINDOW_HEIGHT - 1,
        br_y: WINDOW_WIDTH - 1,
        tl_x: 0,
        tl_y: 0,
        must_split: true,
    });

    for _ in 0..THREAD_COUNT {
        threads.push(task::spawn(work(
            Arc::clone(&bitmaps),
            zoom.clone(),
            Arc::clone(&busy_count),
            Arc::clone(&deque),
        )))
    }

    join_all(threads).await;
    let bitmap = Arc::clone(&bitmaps).lock().await.clone();
    bitmap
}

async fn work(
    bitmaps: Arc<Mutex<Vec<Bitmap>>>,
    zoom: Zoom,
    busy_count: Arc<Mutex<u8>>,
    deque: Arc<Mutex<VecDeque<WorkItem>>>,
) {
    let mut keep_going = true;
    while keep_going {
        let work_item = deque.lock().await.pop_front();
        match work_item {
            None => {
                // println!("STOP {}", count);
                if busy_count.lock().await.eq(&0) {
                    keep_going = false;
                } else {
                    thread::yield_now();
                }
            }
            Some(item) => {
                {
                    let mut lock = busy_count.lock().await;
                    *lock += 1;
                }
                // println!(
                //     "Thread {:?} incremented busy_count to {}",
                //     std::thread::current().id(),
                //     busy_count.load(std::sync::atomic::Ordering::SeqCst)
                // );
                let WorkItem {
                    br_x,
                    br_y,
                    tl_x,
                    tl_y,
                    must_split,
                } = item;
                let width = br_x - tl_x + 1;
                let height = br_y - tl_y + 1;
                let mut must_split = must_split;
                let mut bitmap: Bitmap = Bitmap {
                    pixels: vec![0; (width * height * 4) as usize],
                    size: (width, height),
                    location: (tl_x, tl_y),
                };
                if width <= 8 || height <= 8 {
                    calculate_rectangle(item, &zoom, &mut bitmap);
                } else {
                    for x in tl_x..=br_x {
                        let top_color: Color = get_pixel_color(x, tl_y, &zoom);
                        let bottom_color: Color = get_pixel_color(x, br_y, &zoom);
                        set_pixel_color(x, tl_y, top_color, &mut bitmap);
                        set_pixel_color(x, br_y, bottom_color, &mut bitmap);

                        if top_color != Color::BLACK || bottom_color != Color::BLACK {
                            must_split = true;
                        }
                    }

                    for y in tl_y + 1..=br_y - 1 {
                        let left_color: Color = get_pixel_color(tl_x, y, &zoom);
                        let right_color: Color = get_pixel_color(br_x, y, &zoom);
                        set_pixel_color(tl_x, y, left_color, &mut bitmap);
                        set_pixel_color(br_x, y, right_color, &mut bitmap);

                        if left_color != Color::BLACK || right_color != Color::BLACK {
                            must_split = true;
                        }
                    }

                    if must_split {
                        let mid_x = tl_x + width / 2;
                        let mid_y = tl_y + height / 2;

                        let tl_split = WorkItem {
                            tl_x: tl_x + 1,
                            tl_y: tl_y + 1,
                            br_x: mid_x - 1,
                            br_y: mid_y - 1,
                            must_split: false,
                        };

                        let tr_split = WorkItem {
                            tl_x: mid_x,
                            tl_y: tl_y + 1,
                            br_x: br_x - 1,
                            br_y: mid_y - 1,
                            must_split: false,
                        };

                        let bl_split = WorkItem {
                            tl_x: tl_x + 1,
                            tl_y: mid_y,
                            br_x: mid_x - 1,
                            br_y: br_y - 1,
                            must_split: false,
                        };

                        let br_split = WorkItem {
                            tl_x: mid_x,
                            tl_y: mid_y,
                            br_x: br_x - 1,
                            br_y: br_y - 1,
                            must_split: false,
                        };
                        {
                            let mut lock = deque.lock().await;
                            lock.push_back(tl_split);
                            lock.push_back(tr_split);
                            lock.push_back(bl_split);
                            lock.push_back(br_split);
                        }
                    }
                }
                bitmaps.lock().await.push(bitmap);
                {
                    let mut lock = busy_count.lock().await;
                    *lock -= 1;
                }
                // println!(
                //     "Thread {:?} about to decrement busy_count",
                //     std::thread::current().id()
                // );
            }
        }
    }
}

struct MyWindowHandler {
    zoom_status: Zoom,
    mouse_position: Vector2<f64>,
    bitmaps: Vec<Bitmap>,
    should_clear: bool,
    timings: bool,
}

impl WindowHandler for MyWindowHandler {
    fn on_start(
        &mut self,
        helper: &mut WindowHelper<()>,
        _info: speedy2d::window::WindowStartupInfo,
    ) {
        helper.set_title("Mandelbrot");

        if self.timings {
            test(self, helper);
            return;
        }

        futures::executor::block_on(async {
            self.bitmaps = split_and_spawn(self.zoom_status).await;
            helper.request_redraw();
            // println!(
            //     "{}x loc({}, {}) t: {}",
            //     self.zoom_status.zoom_factor,
            //     self.zoom_status.term_x,
            //     self.zoom_status.term_y,
            //     now.elapsed().as_millis() as f32 / 1000.0
            // );
        });
        // helper.request_redraw();
    }

    fn on_draw(&mut self, _helper: &mut WindowHelper, graphics: &mut Graphics2D) {
        if self.should_clear {
            graphics.clear_screen(Color::BLACK);
        }

        for bitmap in &self.bitmaps {
            let result = graphics.create_image_from_raw_pixels(
                speedy2d::image::ImageDataType::RGBA,
                speedy2d::image::ImageSmoothingMode::Linear,
                bitmap.size,
                &bitmap.pixels,
            );
            match result {
                Ok(image) => graphics
                    .draw_image((bitmap.location.0 as f32, bitmap.location.1 as f32), &image),
                Err(e) => println!("Wrong data type {}", e),
            }
        }
    }

    fn on_mouse_button_down(
        &mut self,
        helper: &mut WindowHelper<()>,
        button: speedy2d::window::MouseButton,
    ) {
        self.should_clear = true;
        println!("{} {}", self.mouse_position.x, self.mouse_position.y);
        let position: Vector2<f64> = pixel_to_coordinate(
            self.mouse_position.x,
            self.mouse_position.y,
            &mut self.zoom_status,
        );
        match button {
            speedy2d::window::MouseButton::Left => {
                self.zoom_status = zoom_to(true, position, &self.zoom_status)
            }
            speedy2d::window::MouseButton::Right => {
                self.zoom_status = zoom_to(false, position, &self.zoom_status)
            }
            speedy2d::window::MouseButton::Middle => {
                self.zoom_status = zoom_to(true, position, &self.zoom_status)
            }
            speedy2d::window::MouseButton::Other(0..=u16::MAX) => {
                self.zoom_status = zoom_to(true, position, &self.zoom_status)
            }
        }

        futures::executor::block_on(async {
            self.bitmaps = split_and_spawn(self.zoom_status).await;
            helper.request_redraw();
        });
    }

    fn on_mouse_move(&mut self, _helper: &mut WindowHelper<()>, position: speedy2d::dimen::Vec2) {
        self.mouse_position.x = position.x as f64;
        self.mouse_position.y = position.y as f64;
    }
}

static POINTS: &[Vector2<f64>] = &[
    Vector2 {
        x: 923.0078125,
        y: 609.1171875,
    },
    Vector2 {
        x: 1028.765625,
        y: 1021.7421875,
    },
    Vector2 {
        x: 964.2265625,
        y: 1026.203125,
    },
    Vector2 {
        x: 1034.9609375,
        y: 1017.640625,
    },
    Vector2 {
        x: 1034.9609375,
        y: 1017.640625,
    },
    Vector2 {
        x: 1034.9609375,
        y: 1017.640625,
    },
    Vector2 {
        x: 1034.9609375,
        y: 1017.640625,
    },
    Vector2 {
        x: 1078.171875,
        y: 1006.5234375,
    },
    Vector2 {
        x: 1102.3828125,
        y: 972.7578125,
    },
    Vector2 {
        x: 987.875,
        y: 993.4453125,
    },
    Vector2 {
        x: 1003.15625,
        y: 1000.0390625,
    },
    Vector2 {
        x: 1047.2421875,
        y: 1053.9765625,
    },
    Vector2 {
        x: 1055.359375,
        y: 1080.0234375,
    },
    Vector2 {
        x: 1055.359375,
        y: 1080.0234375,
    },
    Vector2 {
        x: 1055.359375,
        y: 1080.0234375,
    },
];
