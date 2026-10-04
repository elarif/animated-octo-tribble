import 'package:flutter/material.dart';
import 'package:latlong2/latlong.dart';
import 'package:flutter_map/flutter_map.dart';

import 'sim_connection.dart';

/// Dessine toutes les positions d'agents en un seul canvas (pas de widget
/// par agent : fluide jusqu'à ~1000 agents).
class AgentsPainter extends CustomPainter {
  AgentsPainter(this.agents, this.camera);

  final List<Agent> agents;
  final MapCamera camera;

  static final _carPaint = Paint()..color = const Color(0xFF1565C0);
  static final _pedPaint = Paint()..color = const Color(0xFF2E7D32);

  @override
  void paint(Canvas canvas, Size size) {
    for (final a in agents) {
      final p = camera.latLngToScreenOffset(LatLng(a.lat, a.lon));
      if (p.dx < -8 || p.dy < -8 || p.dx > size.width + 8 || p.dy > size.height + 8) {
        continue;
      }
      canvas.drawCircle(p, a.kind == 'car' ? 3.0 : 2.0, a.kind == 'car' ? _carPaint : _pedPaint);
    }
  }

  @override
  bool shouldRepaint(covariant AgentsPainter old) => true;
}
