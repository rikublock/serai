use std::{thread, time::Duration, collections::HashMap};
use std::{env, str};
use rdkafka::{
  message::ToBytes,
  producer::{BaseProducer, BaseRecord, Producer, ProducerContext, ThreadedProducer},
  ClientConfig, ClientContext, Message, Offset,
};
use message_box::{MessageBox, SecureMessage};
use std::io;

pub fn start() {
  // Creates a producer to send message
  let producer: ThreadedProducer<ProduceCallbackLogger> = ClientConfig::new()
    .set("bootstrap.servers", "localhost:9094")
    .create_with_context(ProduceCallbackLogger {})
    .expect("invalid producer config");

  println!("Sending public keys");

  // Creates a public key message for each coin
  let btc_pub = env::var("BTC_PUB");
  let btc_msg = btc_pub.unwrap();

  // Sends btc pubkey to Kafka
  producer
    .send(BaseRecord::to("public_keys").key(&format!("btc_processor")).payload(&btc_msg))
    .expect("failed to send message");

  let eth_pub = env::var("ETH_PUB");
  let eth_msg = eth_pub.unwrap();

  // Sends eth pubkey to Kafka
  producer
    .send(BaseRecord::to("public_keys").key(&format!("eth_processor")).payload(&eth_msg))
    .expect("failed to send message");

  let xmr_pub = env::var("XMR_PUB");
  let xmr_msg = xmr_pub.unwrap();

  // Sends xmr pubkey to Kafka
  producer
    .send(BaseRecord::to("public_keys").key(&format!("xmr_processor")).payload(&xmr_msg))
    .expect("failed to send message");

  //thread::sleep(Duration::from_secs(10));
  io::stdin().read_line(&mut String::new()).unwrap();
}

struct ProduceCallbackLogger;

impl ClientContext for ProduceCallbackLogger {}

impl ProducerContext for ProduceCallbackLogger {
  type DeliveryOpaque = ();

  fn delivery(
    &self,
    delivery_result: &rdkafka::producer::DeliveryResult<'_>,
    _delivery_opaque: Self::DeliveryOpaque,
  ) {
    let dr = delivery_result.as_ref();
    let msg = dr.unwrap();

    match dr {
      Ok(msg) => {
        let key: &str = msg.key_view().unwrap().unwrap();
        println!(
          "Produced message with key {} in offset {} of partition {}",
          key,
          msg.offset(),
          msg.partition()
        );
      }
      Err(producer_err) => {
        let key: &str = producer_err.1.key_view().unwrap().unwrap();

        println!("failed to produce message with key {} - {}", key, producer_err.0,)
      }
    }
  }
}
