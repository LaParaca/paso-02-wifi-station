// ─── Paso 2: WiFi Station — Conexión WiFi + Provisioning ───
//
// El ESP32 deja de ser un LED parpadeante y se conecta a una red WiFi.
// Si no tiene credenciales guardadas, entra en modo provisioning:
// crea un Access Point donde el usuario configura la red via browser.
//
// Módulos nuevos: wifi, secure_storage, provisioning

// ─── Módulos ───

mod provisioning;
mod secure_storage;
mod wifi;

// ─── Imports ───

use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::gpio::PinDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;

#[allow(unused_imports)]
use esp_idf_svc::sys as _;

use log::{error, info, warn};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secure_storage::SecureStorage;

// ─── Punto de entrada ───
//
// Patrón main() → run(): main() no retorna Result, así que no puede usar ?.
// Delegamos toda la lógica a run() que sí retorna Result.
// Si run() falla, logueamos el error, esperamos 10s y reiniciamos el chip.

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("paso-02-wifi-station");

    if let Err(e) = run() {
        error!("Error fatal: {:?}", e);
        error!("Reiniciando en 10 segundos...");
        std::thread::sleep(Duration::from_secs(10));
        unsafe {
            esp_idf_svc::sys::esp_restart();
        }
    }
}

fn run() -> anyhow::Result<()> {
    // ─── Inicialización del sistema ───

    let peripherals = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;

    // NVS partition — necesaria para SecureStorage (credenciales en flash)
    let nvs_partition = EspDefaultNvsPartition::take()?;

    // LED en GPIO8 — herencia del paso 1, ahora como heartbeat
    let mut led = PinDriver::output(peripherals.pins.gpio8)?;

    info!("LED configurado en GPIO8");

    // ─── Secure Storage ───

    // Arc<Mutex<T>> permite compartir SecureStorage entre main y el
    // handler HTTP del provisioning (que corre en otro thread).
    // Arc = Atomic Reference Counted (shared ownership entre threads)
    // Mutex = exclusión mutua (solo un thread accede a la vez)
    let storage = SecureStorage::new(nvs_partition.clone())?;
    let storage = Arc::new(Mutex::new(storage));

    // ─── Check: ¿Está provisionado? ───

    let is_provisioned = {
        let storage = storage.lock().unwrap();
        storage.is_provisioned()?
    };

    if !is_provisioned {
        // ─── Modo Provisioning ───
        //
        // El dispositivo no tiene credenciales WiFi guardadas.
        // Crear Access Point para que el usuario configure via browser.

        warn!("Device not provisioned!");
        info!("Starting provisioning mode...");
        info!("Connect to WiFi: 'Leonobitech-Setup' / Password: 'setup1234'");
        info!("Then open http://192.168.4.1 in your browser");

        // start_provisioning() NUNCA retorna — reinicia el chip al completar
        provisioning::start_provisioning(peripherals.modem, sysloop, storage)?;

        return Ok(());
    }

    // ─── Modo Normal: Conectar a WiFi ───

    info!("Device is provisioned, loading credentials...");

    let credentials = {
        let storage = storage.lock().unwrap();
        storage.load_credentials()?
    };

    info!("Device ID: {}", credentials.device_id);
    info!("Connecting to WiFi: {}", credentials.wifi_ssid);

    // wifi::connect() retorna Box<EspWifi> — debe mantenerse vivo.
    // Si _wifi se dropea, la conexión WiFi se pierde (RAII).
    let _wifi = wifi::connect(
        &credentials.wifi_ssid,
        &credentials.wifi_password,
        peripherals.modem,
        sysloop,
    )?;

    info!("WiFi connected!");

    // drop() explícito para zeroizar credenciales de memoria.
    // ZeroizeOnDrop sobreescribe los Strings con ceros antes de liberar.
    drop(credentials);
    info!("Credentials zeroized from memory");

    // ─── Loop principal: LED heartbeat ───
    //
    // Herencia del paso 1: el LED parpadea como prueba de vida.
    // En pasos futuros este loop manejará WebSocket messages,
    // telemetría, scheduler, etc.

    info!("Entering main loop...");
    loop {
        led.set_high()?;
        info!("LED ON");
        FreeRtos::delay_ms(500);

        led.set_low()?;
        info!("LED OFF");
        FreeRtos::delay_ms(500);
    }
}
