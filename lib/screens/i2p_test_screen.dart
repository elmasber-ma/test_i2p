import 'package:flutter/material.dart';

import '../services/i2p_service.dart';

/// OPCIONES del router I2P (Ajustes): SOLO configuración y ciclo de vida —
/// estado, iniciar/detener, publicar, sonda SAM y registro. Las pruebas
/// (ejemplo y campos libres) están en el menú radial ("I2P").
class I2pTestScreen extends StatefulWidget {
  const I2pTestScreen({super.key});

  @override
  State<I2pTestScreen> createState() => _I2pTestScreenState();
}

class _I2pTestScreenState extends State<I2pTestScreen> {
  final I2pService _s = I2pService.instance;

  @override
  void initState() {
    super.initState();
    _s.refresh();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('I2P · opciones del router')),
      body: ListenableBuilder(
        listenable: _s,
        builder: (_, __) => ListView(
          padding: const EdgeInsets.all(12),
          children: [
            // ---------- estado
            Card(
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Column(crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                  Row(children: [
                    Icon(
                      Icons.hub,
                      color: _s.running
                          ? (_s.state.contains('listo')
                              ? Colors.green
                              : Colors.orange)
                          : Colors.grey,
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(_s.busy ? 'trabajando…' : _s.state,
                          style: const TextStyle(fontWeight: FontWeight.bold)),
                    ),
                  ]),
                  if (_s.samPort != null)
                    Padding(
                      padding: const EdgeInsets.only(top: 4),
                      child: SelectableText(
                          'SAMv3: 127.0.0.1:${_s.samPort} · directo (sin puente)'),
                    ),
                ]),
              ),
            ),
            // ---------- ciclo de vida
            Row(children: [
              Expanded(
                child: FilledButton.icon(
                  icon: Icon(_s.running
                      ? Icons.stop_rounded
                      : Icons.play_arrow_rounded),
                  label:
                      Text(_s.running ? 'Detener router' : 'Iniciar router'),
                  onPressed: _s.busy
                      ? null
                      : () => _s.running ? _s.stop() : _s.start(),
                ),
              ),
              const SizedBox(width: 8),
              OutlinedButton(
                onPressed: _s.running ? _s.probeSam : null,
                child: const Text('Sonda SAM'),
              ),
            ]),
            // ---------- opción: publicar
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              dense: true,
              title: const Text('Publicar mi dirección (inbound)',
                  style: TextStyle(fontSize: 13)),
              subtitle: const Text(
                  'Solo con IP real + puerto forwardeado; tras CGNAT '
                  'dejalo apagado',
                  style: TextStyle(fontSize: 11)),
              value: _s.publicar,
              onChanged: _s.running
                  ? null
                  : (v) => setState(() => _s.publicar = v),
            ),
            const SizedBox(height: 4),
            const Text(
              'El router queda vivo a nivel app hasta que lo detengas. '
              'Primer arranque: reseed + túneles puede tardar minutos.',
              style: TextStyle(fontSize: 11, color: Colors.grey),
            ),
            const Divider(height: 24),
            // ---------- registro
            if (_s.log.isNotEmpty)
              ExpansionTile(
                initiallyExpanded: true,
                title: const Text('Registro', style: TextStyle(fontSize: 13)),
                childrenPadding: const EdgeInsets.symmetric(horizontal: 12),
                children: [
                  Align(
                    alignment: Alignment.centerLeft,
                    child: SelectableText(
                      _s.log.join('\n'),
                      style: const TextStyle(fontSize: 11),
                    ),
                  ),
                  const SizedBox(height: 12),
                ],
              ),
          ],
        ),
      ),
    );
  }
}
