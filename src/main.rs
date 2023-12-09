use async_std::{
    net::TcpListener,
    net::TcpStream,
    task::{self, JoinHandle},
};
use futures::future::{self, join_all};
use num::complex::Complex;
use speedy2d::{
    color::Color,
    dimen::Vector2,
    // image::{self, ImageHandle},
    window::{WindowHandler, WindowHelper},
    Graphics2D,
    Window,
};
use std::{
    future::{ready, Future, IntoFuture, Ready},
    pin::Pin,
    process::Output,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    thread,
    time::Duration,
    time::Instant,
};
// use std::{fmt::Debug, thread};

const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 1024;
const MAX_DEPTH: u32 = 1000;
const MAX_COLORS: u32 = 16000000;
const COLOR_FACTOR: u32 = MAX_COLORS / MAX_DEPTH;
// const DEBUG: bool = true;
static mut COUNTER: u32 = 0;
// const CROSS: bool = true;

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

fn main() {
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
    });
}

fn calculate_color(c: Complex<f64>) -> Color {
    // unsafe {
    // COUNTER += 1;
    // }
    // let mut x_old = 0.0;
    // let mut y_old = 0.0;
    // let mut x2 = 0.0;
    // let mut y2 = 0.0;
    // let mut w = 0.0;
    let mut depth = 0;
    // let mut period = 0;

    let mut z = Complex::new(0.0, 0.0);
    while z.norm() <= 2.0 && depth < MAX_DEPTH {
        z = z * z + c;
        depth += 1;
    }
    // while x2 + y2 < 4.0 && depth < MAX_DEPTH {
    //     let x = x2 - y2 + c.re;
    //     let y = w - x2 - y2 + c.im;
    //     x2 = x * x;
    //     y2 = y * y;
    //     w = x + y;
    //     w = w * w;
    //     depth += 1;
    // }
    if depth == MAX_DEPTH {
        Color::BLACK
    } else {
        // let m = depth as f64 + 1.0 - w.abs().log2().log10();
        // if depth as f32 / 400.0 > 0.02 {
        // Color::from_hex_rgb(depth * COLOR_FACTOR / 10 + MAX_DEPTH / depth)
        // const n: u32 = 16;
        // let i = MAX_DEPTH / depth;
        // const whiteLevel: u32 = 0x0f0f0f;
        // Color::from_hex_rgb(MAX_DEPTH / depth * 0x0f0f0f + depth * COLOR_FACTOR / 16)
        // Color::from_hex_rgb(depth * COLOR_FACTOR / 10)
        // let z2 = z*z + c;
        // let ln2: f64 = (2.0 as f64).ln();
        // Color::from_hex_rgb(10 * depth + ((1.0 - z2.norm().ln().ln() / ln2) * 10.0) as u32)
        Color::from_hex_rgb(
            1 * (depth) - ((0.0 * z.norm().log10().log10() / 2.0_f64.log10()) as u32),
        )
        // } else {
        // Color::BLACK
        // }
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
    let r = (color.r() * 255.0) as u8;
    let g = (color.g() * 255.0) as u8;
    let b = (color.b() * 255.0) as u8;
    let a = 255 as u8;
    bitmap.pixels[pixel as usize] = r;
    bitmap.pixels[pixel as usize + 1] = g;
    bitmap.pixels[pixel as usize + 2] = b;
    bitmap.pixels[pixel as usize + 3] = a;
}

fn calculate_rectangle(
    tl_x: u32,
    tl_y: u32,
    br_x: u32,
    br_y: u32,
    bitmap: &mut Bitmap,
    zoom: &Zoom,
) {
    for x in tl_x..=br_x {
        for y in tl_y..=br_y {
            let color = get_pixel_color(x, y, zoom);
            set_pixel_color(x, y, color, bitmap);
        }
    }
}

// async fn split_and_spawn(
//     tl_x: u32,
//     tl_y: u32,
//     br_x: u32,
//     br_y: u32,
//     level: u8,
//     zoom: &Zoom,
// ) -> Vec<Bitmap> {
//     let width = br_x - tl_x + 1;
//     let height = br_y - tl_y + 1;
//     let bitmap: Bitmap = Bitmap {
//         pixels: vec![0; (width * height * 4) as usize],
//         size: (width, height),
//         location: (tl_x, tl_y),
//     };
//     let mid_x = tl_x + width / 2;
//     let mid_y = tl_y + height / 2;
//     if level <= 1 {
//         let tasks = vec![
//             task::spawn(split_and_spawn(
// //                 tl_x + 1,
// //                 tl_y + 1,
// //                 mid_x - 1,
// //                 mid_y - 1,
// //                 level + 1,
// //                 zoom,
// //             )),
// //             task::spawn(split_and_spawn(tl_x, tl_y, br_x, br_y, level + 1, zoom)),
// //             task::spawn(split_and_spawn(tl_x, tl_y, br_x, br_y, level + 1, zoom)),
// //             task::spawn(split_and_spawn(tl_x, tl_y, br_x, br_y, level + 1, zoom)),
//         ];
//         let result = join_all(tasks).await;
//         let bitmaps = result.into_iter().flatten().collect::<Vec<Bitmap>>();
//     } else {
//         split(
//             tl_x + 1,
//             tl_y + 1,
//             mid_x - 1,
//             mid_y - 1,
//             level + 1,
//             pixels,
//             zoom,
//         );
//         // println!("\ntop right");
//         split(
//             mid_x,
//             tl_y + 1,
//             br_x - 1,
//             mid_y - 1,
//             level + 1,
//             pixels,
//             zoom,
//         );
//         // println!("\nbottom left");
//         split(
//             tl_x + 1,
//             mid_y,
//             mid_x - 1,
//             br_y - 1,
//             level + 1,
//             pixels,
//             zoom,
//         );
//     }
//     let mut vec = Vec::new();
//     vec.push(bitmap);
//     vec
// }

async fn split_and_spawn(n_x: u32, n_y: u32, zoom: Zoom) -> Vec<Bitmap> {
    let width = WINDOW_WIDTH / n_x;
    let height = WINDOW_HEIGHT / n_y;
    let mut tasks: Vec<JoinHandle<_>> = vec![];
    for x in 0..n_x {
        for y in 0..n_y {
            tasks.push(task::spawn(async move {
                let mut bitmap = Bitmap {
                    pixels: vec![0; (width * height * 4) as usize],
                    size: (width, height),
                    location: (x * width, y * height),
                };
                split(
                    bitmap.location.0,
                    bitmap.location.1,
                    bitmap.location.0 + bitmap.size.0 - 1,
                    bitmap.location.1 + bitmap.size.1 - 1,
                    0,
                    &mut bitmap,
                    &zoom.clone(),
                );
                bitmap
            }));
        }
    }
    future::join_all(tasks).await
}

fn split(tl_x: u32, tl_y: u32, br_x: u32, br_y: u32, level: u8, bitmap: &mut Bitmap, zoom: &Zoom) {
    let width = br_x - tl_x;
    let height = br_y - tl_y;
    let mut must_split = false;
    // 17 is ideal
    if width <= 17 || height <= 17 {
        calculate_rectangle(tl_x, tl_y, br_x, br_y, bitmap, zoom);
        return;
    }
    // let mut color: Color;
    // if level % 2 == 1 {
    //     color = Color::RED;
    // } else if level == 0 {
    //     color = Color::WHITE;
    // } else {
    //     color = Color::BLUE;
    // }
    for x in tl_x..=br_x {
        let top_color: Color = get_pixel_color(x, tl_y, zoom);
        let bottom_color: Color = get_pixel_color(x, br_y, zoom);
        set_pixel_color(x, tl_y, top_color, bitmap);
        set_pixel_color(x, br_y, bottom_color, bitmap);

        if top_color != Color::BLACK || bottom_color != Color::BLACK {
            must_split = true;
        }
    }
    for y in tl_y + 1..=br_y - 1 {
        let left_color: Color = get_pixel_color(tl_x, y, zoom);
        let right_color: Color = get_pixel_color(br_x, y, zoom);
        set_pixel_color(tl_x, y, left_color, bitmap);
        set_pixel_color(br_x, y, right_color, bitmap);

        if left_color != Color::BLACK || right_color != Color::BLACK {
            must_split = true;
        }
    }
    if level == 0 {
        must_split = true;
    }
    if must_split {
        let mid_x = tl_x + width / 2;
        let mid_y = tl_y + height / 2;

        // println!("\ntop left");
        split(
            tl_x + 1,
            tl_y + 1,
            mid_x - 1,
            mid_y - 1,
            level + 1,
            bitmap,
            zoom,
        );
        // println!("\ntop right");
        split(
            mid_x,
            tl_y + 1,
            br_x - 1,
            mid_y - 1,
            level + 1,
            bitmap,
            zoom,
        );
        // println!("\nbottom left");
        split(
            tl_x + 1,
            mid_y,
            mid_x - 1,
            br_y - 1,
            level + 1,
            bitmap,
            zoom,
        );
        // }
        // println!("\nbottom right");
        split(mid_x, mid_y, br_x - 1, br_y - 1, level + 1, bitmap, zoom);
    } else {
        for x in tl_x + 1..br_x - 1 {
            for y in tl_y + 1..br_y - 1 {
                set_pixel_color(x, y, Color::WHITE, bitmap)
            }
        }
    }
}

struct MyWindowHandler {
    zoom_status: Zoom,
    mouse_position: Vector2<f64>,
    bitmaps: Vec<Bitmap>,
    should_clear: bool,
}

impl WindowHandler for MyWindowHandler {
    fn on_start(
        &mut self,
        helper: &mut WindowHelper<()>,
        _info: speedy2d::window::WindowStartupInfo,
    ) {
        helper.set_title("Mandelbrot");

        futures::executor::block_on(async {
            let now = Instant::now();
            self.bitmaps = split_and_spawn(4, 4, self.zoom_status).await;
            helper.request_redraw();
            println!("{}", now.elapsed().as_millis() as f32 / 1000.0);
        });

        // helper.request_redraw();
    }

    fn on_draw(&mut self, _helper: &mut WindowHelper, graphics: &mut Graphics2D) {
        unsafe {
            COUNTER = 0;
        }
        if self.should_clear {
            graphics.clear_screen(Color::BLACK);
        }
        // let mut pixels = vec![0; (WINDOW_WIDTH * WINDOW_WIDTH * 4) as usize];

        for bitmap in &self.bitmaps {
            let result = graphics.create_image_from_raw_pixels(
                speedy2d::image::ImageDataType::RGBA,
                speedy2d::image::ImageSmoothingMode::NearestNeighbor,
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
        let position: Vector2<f64> = pixel_to_coordinate(
            self.mouse_position.x,
            self.mouse_position.y,
            &mut self.zoom_status,
        );
        // println!("({},{})", position.x, position.y);
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
            let now = Instant::now();
            self.bitmaps = split_and_spawn(8, 8, self.zoom_status).await;
            helper.request_redraw();
            unsafe {
                let t = now.elapsed().as_millis() as f32 / 1000.0;
                println!("{} count:{}", t, COUNTER);
            }
        });
    }
    fn on_mouse_move(&mut self, _helper: &mut WindowHelper<()>, position: speedy2d::dimen::Vec2) {
        self.mouse_position.x = position.x as f64;
        self.mouse_position.y = position.y as f64;
    }
    // If desired, on_mouse_move(), on_key_down(), etc...
}
