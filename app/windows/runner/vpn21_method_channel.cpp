// Windows platform channel handler for vpn21.
//
// Implements the "vpn21/native" method channel on Windows.  When
// `requestTun` is called the Dart side receives a stub TUN metadata.
// Actual TUN creation uses the WinTUN driver — the Rust core
// (`tun_desktop`) handles the driver interaction.

#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>
#include <flutter/encodable_value.h>
#include <memory>

void Vpn21RegisterMethodChannel(
    flutter::BinaryMessenger* messenger) {
  auto channel = std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
      messenger, "vpn21/native",
      &flutter::StandardMethodCodec::GetInstance());

  channel->SetMethodCallHandler(
      [](const flutter::MethodCall<flutter::EncodableValue>& call,
         std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
        if (call.method_name() == "requestTun") {
          flutter::EncodableMap tun;
          tun[flutter::EncodableValue("fd")] = flutter::EncodableValue(-1);
          tun[flutter::EncodableValue("mtu")] = flutter::EncodableValue(1500);
          tun[flutter::EncodableValue("ipv4")] =
              flutter::EncodableValue("10.19.21.1");
          tun[flutter::EncodableValue("mask")] = flutter::EncodableValue(24);
          tun[flutter::EncodableValue("dnsPort")] = flutter::EncodableValue(53);
          result->Success(flutter::EncodableValue(tun));
        } else if (call.method_name() == "releaseTun") {
          result->Success();
        } else {
          result->NotImplemented();
        }
      });
}
