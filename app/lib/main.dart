import 'package:flutter/material.dart';

import 'sim_connection.dart';
import 'sim_map.dart';

void main() {
  // URL du backend : override au build avec
  //   flutter build web --dart-define=SIM_WS_URL=wss://mon-serveur/sim
  const simUrl = String.fromEnvironment(
    'SIM_WS_URL',
    defaultValue: 'ws://localhost:9000/sim',
  );
  runApp(const ComorosTrafficApp(simUrl: simUrl));
}

class ComorosTrafficApp extends StatefulWidget {
  const ComorosTrafficApp({super.key, required this.simUrl});

  final String simUrl;

  @override
  State<ComorosTrafficApp> createState() => _ComorosTrafficAppState();
}

class _ComorosTrafficAppState extends State<ComorosTrafficApp> {
  late final SimConnection _sim = SimConnection(url: widget.simUrl);

  @override
  void dispose() {
    _sim.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Comores — simulation de trafic',
      theme: ThemeData(useMaterial3: true, colorSchemeSeed: Colors.teal),
      home: Scaffold(
        appBar: AppBar(title: const Text('Comores — simulation de trafic')),
        body: SimMap(sim: _sim),
      ),
    );
  }
}
