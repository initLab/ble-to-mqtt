use std::error::Error;
use std::io::Write;
use std::time::Duration;

use anyhow::Context;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral, ScanFilter};
use btleplug::platform::{Adapter, Manager};
use chrono::Local;
use envconfig::Envconfig;
use futures::stream::StreamExt;
use log::{error, info, LevelFilter, warn};
use mqtt::properties;
use paho_mqtt as mqtt;
use paho_mqtt::{AsyncClient, Topic};
use paho_mqtt::Error::PahoDescr;
use serde_json::to_vec;
use tokio::time;

use config::Config;
use events::{*};

mod config;
mod events;

const MQTT_CLIENT_DISCONNECTED: i32 = -3;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::builder()
        .format(|buf, record| {
            writeln!(
                buf,
                "[{}] {}: {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter_level(LevelFilter::Info)
        .init();


    info!("Initializing configuration");
    let config = Config::init_from_env()?;

    info!("Connecting to mqtt broker mqtt://{}:{} ..", config.mqtt_host, config.mqtt_port);
    let mqtt_client = mqtt_init(&config).await?;

    let adapter = get_adapter().await?;
    let mut events = adapter.events().await?;
    adapter.start_scan(ScanFilter::default()).await?;

    info!("Scanning for ble events..");
    while let Some(event) = events.next().await {
        match process_central_event(&config, &adapter, event).await {
            Ok((payload, topic_name)) => {
                info!("topic: {}", &topic_name);
                let topic = Topic::new(&mqtt_client, topic_name, config.mqtt_topic_qos.unwrap_or_default());
                publish_to_topic(&mqtt_client, topic, &payload).await
            }
            Err(err) => error!("Failed to process central event: {:?}", err)
        }
    }

    Ok(())
}

async fn get_adapter() -> anyhow::Result<Adapter> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    return Ok(adapters.into_iter().nth(0).context("no adapter")?);
}

async fn process_central_event(config: &Config, adapter: &Adapter, event: CentralEvent) -> anyhow::Result<(Vec<u8>, String)> {
    let topic: String;
    let payload = match event {
        CentralEvent::DeviceDiscovered(id) => {
            let (name, rssi) = get_properties(adapter, &id).await?;
            topic = format!("{}/{}/{}", config.mqtt_topic, "DeviceDiscovered", name.clone().unwrap_or(id.to_string()));
            to_vec(&Event::new_simple("DeviceDiscovered".into(), name, rssi, id.to_string()))?
        }
        CentralEvent::DeviceUpdated(id) => {
            let (name, rssi) = get_properties(adapter, &id).await?;
            topic = format!("{}/{}/{}", config.mqtt_topic, "DeviceUpdated", name.clone().unwrap_or(id.to_string()));
            to_vec(&Event::new_simple("DeviceUpdated".into(), name, rssi, id.to_string()))?
        }
        CentralEvent::DeviceConnected(id) => {
            let (name, rssi) = get_properties(adapter, &id).await?;
            topic = format!("{}/{}/{}", config.mqtt_topic, "DeviceConnected", name.clone().unwrap_or(id.to_string()));
            to_vec(&Event::new_simple("DeviceConnected".into(), name, rssi, id.to_string()))?
        }
        CentralEvent::DeviceDisconnected(id) => {
            let (name, rssi) = get_properties(adapter, &id).await?;
            topic = format!("{}/{}/{}", config.mqtt_topic, "DeviceDisconnected", name.clone().unwrap_or(id.to_string()));
            to_vec(&Event::new_simple("DeviceDisconnected".into(), name, rssi, id.to_string()))?
        }
        CentralEvent::ManufacturerDataAdvertisement { id, manufacturer_data } => {
            let (name, rssi) = get_properties(adapter, &id).await?;
            topic = format!("{}/{}/{}", config.mqtt_topic, "ManufacturerDataAdvertisement", name.clone().unwrap_or(id.to_string()));
            let data = manufacturer_data.iter().
                map(|(k, v)| (k.clone(), hex::encode(v))).
                collect();
            to_vec(&ManufacturerDataAdvertisementEvent::new(name, rssi, id.to_string(), data))?
        }
        CentralEvent::ServiceDataAdvertisement { id, service_data } => {
            let (name, rssi) = get_properties(adapter, &id).await?;
            topic = format!("{}/{}/{}", config.mqtt_topic, "ServiceDataAdvertisement", name.clone().unwrap_or(id.to_string()));
            let data = service_data.iter().
                map(|(k, v)| (k.clone(), hex::encode(v))).
                collect();
            to_vec(&ServiceDataAdvertisementEvent::new(name, rssi, id.to_string(), data))?
        }
        CentralEvent::ServicesAdvertisement { id, services } => {
            let (name, rssi) = get_properties(adapter, &id).await?;
            topic = format!("{}/{}/{}", config.mqtt_topic, "ServicesAdvertisement", name.clone().unwrap_or(id.to_string()));
            to_vec(&ServicesAdvertisementEvent::new(name, rssi, id.to_string(), services))?
        }
    };
    Ok((payload, topic))
}

async fn get_properties(adapter: &Adapter, id: &btleplug::platform::PeripheralId) -> anyhow::Result<(Option<String>, Option<i16>)> {
    let peripheral = adapter.peripheral(&id).await?;
    if let Some(properties) = peripheral.properties().await? {
        Ok((properties.local_name, properties.rssi))
    }
    Ok((None, None))
}

async fn mqtt_init(config: &Config) -> anyhow::Result<AsyncClient> {
    let create_opts = mqtt::CreateOptionsBuilder::new()
        .server_uri(format!("mqtt://{}:{}", config.mqtt_host, config.mqtt_port))
        .client_id(config.mqtt_client_id.clone().unwrap_or_default())
        .finalize();

    let mqtt_client = AsyncClient::new(create_opts)?;

    let props = properties! {
        mqtt::PropertyCode::SessionExpiryInterval => 86400,
    };

    let conn_opts = mqtt::ConnectOptionsBuilder::new_v5()
        .keep_alive_interval(Duration::from_secs(config.mqtt_keep_alive_interval_seconds))
        .clean_start(config.mqtt_clean_start)
        .properties(props)
        .user_name(config.mqtt_username.clone().unwrap_or_default())
        .password(config.mqtt_password.clone().unwrap_or_default())
        .finalize();

    mqtt_client.connect(conn_opts).await?;

    Ok(mqtt_client)
}

async fn publish_to_topic<'a>(mqtt_client: &AsyncClient, topic: Topic<'a>, payload: &Vec<u8>) {
    for _ in 1..=30 {
        match topic.publish(payload.to_owned()).await {
            Ok(_) => { return; }
            Err(err) => match err {
                PahoDescr(id, reason) => {
                    if id != MQTT_CLIENT_DISCONNECTED {
                        error!("Failed to publish message id: {:?} reason: {:?}", id, reason);
                        time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }

                    warn!("Connection to the mqtt broker was lost, attempting to reconnect..");
                    match mqtt_client.reconnect().await {
                        Ok(_) => info!("Reconnected"),
                        Err(reconnect_err) => {
                            error!("Failed to reconnect: {:?}", reconnect_err);
                            time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
                _ => {
                    error!("Failed to publish message: {:?}", err);
                    time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}
