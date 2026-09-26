use calvin_macros::{calvin_storage_group, hlog, hstore};

calvin_storage_group!(WeatherMonitor);

#[repr(C)]
struct SensorEvent {
    temp: f64,
    humidity: f64,
}

fn main() {
    println!("Host App starting...");

    let event = SensorEvent {
        temp: 72.5,
        humidity: 45.0,
    };
    hstore!(WeatherMonitor, event);
    println!("Recorded event via HSTORE.");

    let temp = 72.5f64;
    let humidity = 45.0f64;
    hlog!(WeatherMonitor, "Temp: $0, Humidity: $1", temp, humidity);
    println!("Recorded event via HLOG.");
}
