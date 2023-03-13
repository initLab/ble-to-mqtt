use envconfig::Envconfig;

#[derive(Envconfig)]
pub struct Config {
    #[envconfig(from = "BLE_TO_MQTT_HOST")]
    pub mqtt_host: String,

    #[envconfig(from = "BLE_TO_MQTT_PORT", default = "1883")]
    pub mqtt_port: u16,

    #[envconfig(from = "BLE_TO_MQTT_USERNAME")]
    pub mqtt_username: Option<String>,

    #[envconfig(from = "BLE_TO_MQTT_PASSWORD")]
    pub mqtt_password: Option<String>,

    #[envconfig(from = "BLE_TO_MQTT_CLIENT_ID")]
    pub mqtt_client_id: Option<String>,

    #[envconfig(from = "BLE_TO_MQTT_TOPIC")]
    pub mqtt_topic: String,

    #[envconfig(from = "BLE_TO_MQTT_TOPIC_QOS")]
    pub mqtt_topic_qos: Option<i32>,

    #[envconfig(from = "BLE_TO_MQTT_KEEP_ALIVE_INTERVAL_SEC", default = "10")]
    pub mqtt_keep_alive_interval_seconds: u64,

    #[envconfig(from = "BLE_TO_MQTT_CLEAN_START", default = "false")]
    pub mqtt_clean_start: bool,
}
