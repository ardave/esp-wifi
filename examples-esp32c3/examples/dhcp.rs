#![no_std]
#![no_main]

use examples_util::hal;

use embedded_io::blocking::*;
use embedded_svc::ipv4::Interface;
use embedded_svc::wifi::{AccessPointInfo, ClientConfiguration, Configuration, Wifi};

use esp_backtrace as _;
use esp_println::logger::init_logger;
use esp_println::{print, println};
use esp_wifi::wifi::utils::create_network_interface;
use esp_wifi::wifi::{WifiError, WifiMode};
use esp_wifi::wifi_interface::WifiStack;
use esp_wifi::{current_millis, initialize};
use hal::clock::{ClockControl, CpuClock};
use hal::Rng;
use hal::{peripherals::Peripherals, prelude::*, Rtc};
use smoltcp::iface::SocketStorage;
use smoltcp::wire::Ipv4Address;

//use embedded_hal::blocking::delay::DelayMs;
use esp32c3_hal::delay::Delay;

use esp32c3_hal::i2c::I2C;
use esp32c3_hal::IO;
use bme280::BME280;


const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[entry]
fn main() -> ! {
    init_logger(log::LevelFilter::Info);
    esp_wifi::init_heap();

    let peripherals = Peripherals::take();

    let mut system = examples_util::system!(peripherals);
    let clocks = examples_util::clocks!(system);
    examples_util::rtc!(peripherals);

    let timer = examples_util::timer!(peripherals, clocks);

    let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);

    let i2c_bus = I2C::new(
        peripherals.I2C0,
        io.pins.gpio5,
        io.pins.gpio6,
        100u32.kHz(),
        &mut system.peripheral_clock_control,
        &clocks,
    );

    let delay = Delay::new(&clocks);

    let mut bme280 = BME280::new_secondary(i2c_bus, delay);

    let bme_init_result = bme280.init();

    match &bme_init_result {
        Ok(_) => {
            let measurements_result = bme280.measure();
            match measurements_result {
                Ok(measurements) => println!("Measurements: {:?}", measurements),
                Err(e) => println!("Measurements Errors: {:?}", e)
            }
            //println!("Measurements: {:?}", measurements);
        },

        Err(e) => println!("BME280 Initialization Error: {:?}", e)
    }
    
    println!("Dave: {:?}", bme_init_result);

    initialize(
        timer,
        Rng::new(peripherals.RNG),
        system.radio_clock_control,
        &clocks,
    )
    .unwrap();

    let (wifi, _) = peripherals.RADIO.split();
    let mut socket_set_entries: [SocketStorage; 3] = Default::default();
    let (iface, device, mut controller, sockets) =
        create_network_interface(wifi, WifiMode::Sta, &mut socket_set_entries);
    let wifi_stack = WifiStack::new(iface, device, sockets, current_millis);

    let client_config = Configuration::Client(ClientConfiguration {
        ssid: SSID.into(),
        password: PASSWORD.into(),
        ..Default::default()
    });
    let res = controller.set_configuration(&client_config);
    println!("wifi_set_configuration returned {:?}", res);

    controller.start().unwrap();
    println!("is wifi started: {:?}", controller.is_started());

    println!("Start Wifi Scan");
    let res: Result<(heapless::Vec<AccessPointInfo, 10>, usize), WifiError> = controller.scan_n();
    if let Ok((res, _count)) = res {
        for ap in res {
            println!("{:?}", ap);
        }
    }

    println!("{:?}", controller.get_capabilities());
    println!("wifi_connect {:?}", controller.connect());

    // wait to get connected
    println!("Wait to get connected");
    loop {
        let res = controller.is_connected();
        match res {
            Ok(connected) => {
                if connected {
                    break;
                }
            }
            Err(err) => {
                println!("{:?}", err);
                loop {}
            }
        }
    }
    println!("{:?}", controller.is_connected());

    // wait for getting an ip address
    println!("Wait to get an ip address");
    loop {
        wifi_stack.work();

        if wifi_stack.is_iface_up() {
            println!("got ip {:?}", wifi_stack.get_ip_info());
            break;
        }
    }

    println!("Start busy loop on main");

    let mut rx_buffer = [0u8; 1536];
    let mut tx_buffer = [0u8; 1536];
    let mut socket = wifi_stack.get_socket(&mut rx_buffer, &mut tx_buffer);

    loop {
        println!("Making HTTP request");
        socket.work();

        socket
            .open(Ipv4Address::new(142, 250, 185, 115), 80)
            .unwrap();

        socket
            .write(b"GET / HTTP/1.0\r\nHost: www.mobile-j.de\r\n\r\n")
            .unwrap();
        socket.flush().unwrap();

        let wait_end = current_millis() + 20 * 1000;
        loop {
            let mut buffer = [0u8; 512];
            if let Ok(len) = socket.read(&mut buffer) {
                let to_print = unsafe { core::str::from_utf8_unchecked(&buffer[..len]) };
                print!("{}", to_print);
            } else {
                break;
            }

            if current_millis() > wait_end {
                println!("Timeout");
                break;
            }
        }
        println!();

        socket.disconnect();

        let wait_end = current_millis() + 5 * 1000;
        while current_millis() < wait_end {
            socket.work();
        }
    }
}
