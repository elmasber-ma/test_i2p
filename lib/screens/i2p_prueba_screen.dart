import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import '../services/i2p_service.dart';

/// PRUEBA de I2P (menú radial): demo con ejemplo fijo + campos libres para
/// GET y descarga. La CONFIGURACIÓN del router está aparte, en Ajustes.
class I2pPruebaScreen extends StatefulWidget {
  const I2pPruebaScreen({super.key});

  @override
  State<I2pPruebaScreen> createState() => _I2pPruebaScreenState();
}

class _I2pPruebaScreenState extends State<I2pPruebaScreen> {
  final I2pService _s = I2pService.instance;
  final _getCtrl = TextEditingController();
  final _dlCtrl = TextEditingController();

  /// Eepsite canónico (mantenido por zzz, dev core de I2P).
  static const _urlEjemplo = 'http://stats.i2p/';

  String _salida = '';
  double? _progreso;

  @override
  void dispose() {
    _getCtrl.dispose();
    _dlCtrl.dispose();
    super.dispose();
  }

  Future<void> _ejemplo() async {
    setState(() => _salida = '[ejemplo] consultando $_urlEjemplo …');
    try {
      final r = await _s.httpGet(_urlEjemplo);
      setState(() =>
          _salida = '[ejemplo]\n' +
              (r.length > 3000 ? '${r.substring(0, 3000)}…' : r));
    } catch (e) {
      setState(() => _salida = '[ejemplo] ERROR: $e\n'
          '(si recién arrancó el router, esperá a que construya túneles '
          'y reintentá)');
    }
  }

  Future<void> _get() async {
    final url = _getCtrl.text.trim();
    setState(() => _salida = '[GET] $url …');
    try {
      final r = await _s.httpGet(url);
      setState(() =>
          _salida = r.length > 4000 ? '${r.substring(0, 4000)}…' : r);
    } catch (e) {
      setState(() => _salida = 'ERROR: $e');
    }
  }

  Future<void> _descargar() async {
    final url = _dlCtrl.text.trim();
    try {
      final dir = await getApplicationSupportDirectory();
      final path =
          '${dir.path}/i2p_dl_${DateTime.now().millisecondsSinceEpoch}';
      setState(() {
        _progreso = null;
        _salida = '[descarga] $url …';
      });
      await _s.download(url, savePath: path,
          onProgress: (got) =>
              mounted ? setState(() => _progreso = got.toDouble()) : null);
      setState(() {
        _progreso = -1;
        _salida = '[descarga] guardado en:\n$path';
      });
    } catch (e) {
      setState(() {
        _progreso = null;
        _salida = 'ERROR descarga: $e';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('I2P · prueba')),
      body: ListenableBuilder(
        listenable: _s,
        builder: (_, __) => ListView(
          padding: const EdgeInsets.all(12),
          children: [
            Row(children: [
              Icon(Icons.hub,
                  color: _s.running ? Colors.green : Colors.grey, size: 18),
              const SizedBox(width: 6),
              Text(_s.running ? 'router corriendo' : 'router apagado',
                  style: const TextStyle(fontSize: 12)),
              const Spacer(),
              if (!_s.running && !_s.busy)
                FilledButton.tonalIcon(
                  icon: const Icon(Icons.play_arrow_rounded, size: 16),
                  label: const Text('Iniciar'),
                  onPressed: _s.start,
                ),
            ]),
            const Divider(height: 24),
            // ---------- EJEMPLO fijo
            Card(
              color: Colors.blueGrey.shade900,
              margin: EdgeInsets.zero,
              child: ListTile(
                dense: true,
                leading: const Icon(Icons.science_outlined),
                title: const Text('Ejemplo',
                    style: TextStyle(fontWeight: FontWeight.bold)),
                subtitle: const Text('GET $_urlEjemplo · eepsite estable',
                    style: TextStyle(fontSize: 11)),
                trailing: OutlinedButton(
                  onPressed: _s.running ? _ejemplo : null,
                  child: const Text('Correr'),
                ),
              ),
            ),
            const Divider(height: 24),
            // ---------- GET libre
            TextField(
              controller: _getCtrl,
              keyboardType: TextInputType.url,
              decoration: const InputDecoration(
                labelText: 'URL para GET (nombre.i2p / b32 / base64)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
              onSubmitted: (_) => _s.running ? _get() : null,
            ),
            const SizedBox(height: 8),
            OutlinedButton.icon(
              icon: const Icon(Icons.travel_explore),
              label: const Text('GET'),
              onPressed: _s.running && _getCtrl.text.isNotEmpty ? _get : null,
            ),
            const Divider(height: 24),
            // ---------- descarga libre
            TextField(
              controller: _dlCtrl,
              keyboardType: TextInputType.url,
              decoration: const InputDecoration(
                labelText: 'URL de archivo a descargar por I2P',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 8),
            OutlinedButton.icon(
              icon: const Icon(Icons.download_rounded),
              label: const Text('Descargar'),
              onPressed:
                  _s.running && _dlCtrl.text.isNotEmpty ? _descargar : null,
            ),
            if (_progreso != null)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: LinearProgressIndicator(value: null),
              ),
            const Divider(height: 24),
            if (_salida.isNotEmpty)
              Card(
                color: Colors.black87,
                child: Padding(
                  padding: const EdgeInsets.all(10),
                  child: SelectableText(
                    _salida,
                    style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 11.5,
                        color: Colors.lightGreenAccent),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
