import 'package:flutter/material.dart';

import 'sim_connection.dart';
import 'sim_map.dart';

void main() {
  // En web l'app est servie sur localhost → localhost:9000 marche.
  // Sur desktop Linux, idem.
  const defaultUrl = 'ws://localhost:9000/sim';
  runApp(ComorosTrafficApp(simUrl: defaultUrl));
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
