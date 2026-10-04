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

class _Island {
  const _Island(this.name, this.center, this.zoom, this.minZoom, this.bounds);
  final String name;
  final LatLng center;
  final double zoom;
  final double minZoom;
  final LatLngBounds bounds;
}

final _islands = <_Island>[
  _Island(
    'Grande Comore',
    LatLng(-11.65, 43.37),
    11.2,
    10.5,
    LatLngBounds(LatLng(-12.0, 43.15), LatLng(-11.25, 43.6)),
  ),
  _Island(
    'Mohéli',
    LatLng(-12.32, 43.75),
    11.5,
    11.0,
    LatLngBounds(LatLng(-12.50, 43.55), LatLng(-12.15, 43.95)),
  ),
  _Island(
    'Anjouan',
    LatLng(-12.23, 44.38),
    11.0,
    10.5,
    LatLngBounds(LatLng(-12.50, 44.12), LatLng(-12.0, 44.65)),
  ),
];

class _SimMapState extends State<SimMap> {
  final _mapController = MapController();
  int _island = 0;

  @override
  Widget build(BuildContext context) {
    final island = _islands[_island];
    return Column(
      children: [
        Expanded(
          child: Stack(
            children: [
              FlutterMap(
                key: ValueKey(_island),
                mapController: _mapController,
                options: MapOptions(
                  initialCenter: island.center,
                  initialZoom: island.zoom,
                  minZoom: island.minZoom,
                  maxZoom: 16.0,
                  cameraConstraint: CameraConstraint.contain(bounds: island.bounds),
                ),
                children: [
                  TileLayer(
                    urlTemplate: 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
                    userAgentPackageName: 'com.example.comores_traffic_app',
                  ),
                ],
              ),
              // Sélecteur d'île en haut, au centre
              Positioned(
                top: 8,
                left: 0,
                right: 0,
                child: Center(
                  child: Material(
                    elevation: 4,
                    borderRadius: BorderRadius.circular(20),
                    child: DropdownButtonHideUnderline(
                      child: DropdownButton<int>(
                        value: _island,
                        borderRadius: BorderRadius.circular(12),
                        items: [
                          for (var i = 0; i < _islands.length; i++)
                            DropdownMenuItem(value: i, child: Text(_islands[i].name)),
                        ],
                        padding: const EdgeInsets.symmetric(horizontal: 16),
                        onChanged: (v) => setState(() => _island = v ?? 0),
                      ),
                    ),
                  ),
                ),
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
