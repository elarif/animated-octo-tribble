import 'package:flutter/material.dart';
import 'package:flutter_map/flutter_map.dart';
import 'package:latlong2/latlong.dart';

import 'agents_painter.dart';
import 'sim_connection.dart';

/// Carte Comores (tuiles OSM) + canvas des agents simulés.
class SimMap extends StatefulWidget {
  const SimMap({super.key, required this.sim});

  final SimConnection sim;

  @override
  State<SimMap> createState() => _SimMapState();
}

class _SimMapState extends State<SimMap> {
  final _mapController = MapController();

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Expanded(
          child: Stack(
            children: [
              FlutterMap(
                mapController: _mapController,
                options: MapOptions(
                  initialCenter: const LatLng(-12.0, 44.0),
                  initialZoom: 7.0,
                ),
                children: [
                  TileLayer(
                    urlTemplate: 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
                    userAgentPackageName: 'com.example.comores_traffic_app',
                  ),
                ],
              ),
              // Canvas des agents par-dessus les tuiles : on repeint quand la
              // caméra bouge OU quand un nouveau tick arrive.
              Positioned.fill(
                child: IgnorePointer(
                  child: StreamBuilder(
                    stream: _mapController.mapEventStream,
                    builder: (context, _) {
                      return AnimatedBuilder(
                        animation: widget.sim,
                        builder: (context, _) {
                          final camera = _mapController.camera;
                          return CustomPaint(
                            painter: AgentsPainter(widget.sim.agents, camera),
                            size: Size.infinite,
                          );
                        },
                      );
                    },
                  ),
                ),
              ),
            ],
          ),
        ),
        _StatusBar(sim: widget.sim),
      ],
    );
  }
}

class _StatusBar extends StatelessWidget {
  const _StatusBar({required this.sim});

  final SimConnection sim;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      color: Theme.of(context).colorScheme.inverseSurface,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: AnimatedBuilder(
        animation: sim,
        builder: (context, _) {
          final color = Theme.of(context).colorScheme.onInverseSurface;
          return DefaultTextStyle(
            style: TextStyle(color: color, fontSize: 14),
            child: Row(
              children: [
                Icon(sim.connected ? Icons.cloud_done : Icons.cloud_off,
                    color: color, size: 16),
                const SizedBox(width: 8),
                const Icon(Icons.change_history, color: Color(0xFF90CAF9), size: 14),
                const SizedBox(width: 4),
                Text('Voitures: ${sim.cars}'),
                const SizedBox(width: 16),
                const Icon(Icons.circle, color: Color(0xFF81C784), size: 12),
                const SizedBox(width: 4),
                Text('Piétons: ${sim.pedestrians}'),
                const SizedBox(width: 16),
                Text('Tick: ${sim.tick}'),
              ],
            ),
          );
        },
      ),
    );
  }
}
