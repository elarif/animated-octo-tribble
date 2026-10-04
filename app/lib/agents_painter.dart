import 'dart:math' show cos, sin;

import 'package:flutter/material.dart';
import 'package:latlong2/latlong.dart' show LatLng;
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
      if (a.kind == 'car') {
        _drawCar(canvas, p, a.heading);
      } else {
        canvas.drawCircle(p, 2.0, _pedPaint);
      }
    }
  }

  /// Voiture = triangle bleu orienté selon la direction du déplacement.
  /// `headingDeg` est un cap boussole (0° = nord, 90° = est), converti en
  /// angle écran (0° = est, y vers le bas) : angle = hdg - 90°.
  void _drawCar(Canvas canvas, Offset c, double headingDeg) {
    const r = 5.0;
    final th = (headingDeg - 90) * 3.141592653589793 / 180.0;
    final forward = Offset(r * cos(th), r * sin(th));
    final backLeft = Offset(-r * 0.6 * cos(th) - r * 0.55 * sin(th),
        -r * 0.6 * sin(th) + r * 0.55 * cos(th));
    final backRight = Offset(-r * 0.6 * cos(th) + r * 0.55 * sin(th),
        -r * 0.6 * sin(th) - r * 0.55 * cos(th));
    final path = Path()
      ..moveTo(c.dx + forward.dx, c.dy + forward.dy)
      ..lineTo(c.dx + backLeft.dx, c.dy + backLeft.dy)
      ..lineTo(c.dx + backRight.dx, c.dy + backRight.dy)
      ..close();
    canvas.drawPath(path, _carPaint);
  }

  @override
  bool shouldRepaint(covariant AgentsPainter old) => true;
}
