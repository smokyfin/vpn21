// Linux platform channel handler for vpn21.
//
// Implements the "vpn21/native" method channel on Linux.  When `requestTun`
// is called the Dart side receives a stub TUN metadata.  Actual TUN creation
// happens in the Rust core (`tun_desktop::create_tun_linux`) — the helper
// binary that runs with CAP_NET_ADMIN creates the device and passes its fd
// back through the Rust FFI.

#include <flutter_linux/flutter_linux.h>

static void vpn21_method_call_handler(FlMethodChannel* channel,
                                      FlMethodCall* method_call,
                                      gpointer user_data) {
    const gchar* method = fl_method_call_get_name(method_call);
    if (g_strcmp0(method, "requestTun") == 0) {
        g_autoptr(FlValue) result = fl_value_new_map();
        fl_value_set_string_take(result, "fd", fl_value_new_int(-1));
        fl_value_set_string_take(result, "mtu", fl_value_new_int(1500));
        fl_value_set_string_take(result, "ipv4",
                                 fl_value_new_string("10.19.21.1"));
        fl_value_set_string_take(result, "mask", fl_value_new_int(24));
        fl_value_set_string_take(result, "dnsPort", fl_value_new_int(53));
        fl_method_call_respond_success(method_call, result, NULL);
    } else if (g_strcmp0(method, "releaseTun") == 0) {
        fl_method_call_respond_success(method_call, fl_value_new_null(), NULL);
    } else {
        fl_method_call_respond_not_implemented(method_call, NULL);
    }
}

void vpn21_register_method_channel(FlBinaryMessenger* messenger) {
    g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
    FlMethodChannel* channel = fl_method_channel_new(
        messenger, "vpn21/native",
        FL_METHOD_CODEC(codec));
    fl_method_channel_set_method_call_handler(
        channel, vpn21_method_call_handler, NULL, NULL);
}
