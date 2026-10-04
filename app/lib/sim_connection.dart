import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

class Agent {
  final int id;
  final String kind; // "car" | "pedestrian"
  final double lat;
  final double lon;
  final double heading;

  const Agent({
    required this.id,
    required this.kind,
    required this.lat,
    required this.lon,
    required this.heading,
  });

  factory Agent.fromJson(Map<String, dynamic> j) => Agent(
        id: j['id'] as int,
        kind: j['k'] as String,
        lat: (j['lat'] as num).toDouble(),
        lon: (j['lon'] as num).toDouble(),
        heading: (j['hdg'] as num?)?.toDouble() ?? 0.0,
      );
}

/// Connexion unique au WebSocket `/sim` du backend Rust.
/// Les ticks alimentent [agents]/[tick] et notifient les écouteurs.
class SimConnection extends ChangeNotifier {
  final String url;

  SimConnection({this.url = 'ws://localhost:9000/sim'}) {
    connect();
  }

  List<Agent> agents = const [];
  int tick = 0;
  int cars = 0;
  int pedestrians = 0;
  bool connected = false;

  WebSocketChannel? _channel;
  Timer? _reconnectTimer;
  bool _disposed = false;

  void connect() {
    if (_disposed) return;
    try {
      _channel = WebSocketChannel.connect(Uri.parse(url));
      connected = true;
      notifyListeners();
      _channel!.stream.listen(
        (data) {
          try {
            final j = jsonDecode(data as String) as Map<String, dynamic>;
            final list = (j['agents'] as List?) ?? const [];
            agents = [
              for (final a in list)
                Agent.fromJson(a as Map<String, dynamic>),
            ];
            tick = j['t'] as int? ?? tick;
            cars = agents.where((a) => a.kind == 'car').length;
            pedestrians = agents.where((a) => a.kind != 'car').length;
            notifyListeners();
          } catch (e) {
            debugPrint('sim frame invalide: $e');
          }
        },
        onError: (_) => _scheduleReconnect(),
        onDone: _scheduleReconnect,
      );
    } catch (_) {
      _scheduleReconnect();
    }
  }

  void _scheduleReconnect() {
    if (_disposed) return;
    connected = false;
    notifyListeners();
    _reconnectTimer?.cancel();
    _reconnectTimer = Timer(const Duration(seconds: 2), connect);
  }

  @override
  void dispose() {
    _disposed = true;
    _reconnectTimer?.cancel();
    _channel?.sink.close();
    super.dispose();
  }
}
