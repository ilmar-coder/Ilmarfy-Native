package br.com.ilmar.ilmarfy.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import br.com.ilmar.ilmarfy.core.Core
import br.com.ilmar.ilmarfy.core.Updater
import br.com.ilmar.ilmarfy.playback.PlayerConnection
import kotlinx.coroutines.launch
import java.text.DateFormat
import java.util.Date

@Composable
fun SettingsScreen(nav: NavHostController, onMessage: (String) -> Unit) {
    val version by Core.version.collectAsState()
    var trackCount by remember { mutableStateOf(0u) }
    var lastSync by remember { mutableStateOf<Long?>(null) }
    var offlineCount by remember { mutableStateOf(0) }
    var offlineBytes by remember { mutableStateOf(0uL) }
    var confirmLogout by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(version) {
        runCatching {
            Core.io { c ->
                trackCount = c.trackCount()
                lastSync = c.lastSync()
                offlineCount = c.downloadedIds().size
                offlineBytes = c.downloadsSize()
            }
        }
    }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Rounded.ArrowBack, "Voltar") }
            Text("Configurações", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
        }
        Section("Conta") {
            Info("Servidor", Core.server)
            Info("Este aparelho", Core.deviceName ?: "—")
        }
        Section("Biblioteca") {
            Info("Faixas", "$trackCount")
            Info("Última sincronização", lastSync?.let { DateFormat.getDateTimeInstance().format(Date(it * 1000)) } ?: "nunca")
            OutlinedButton(onClick = { scope.launch { onMessage(syncNow() ?: "Biblioteca atualizada") } }, modifier = Modifier.fillMaxWidth()) {
                Text("Sincronizar agora")
            }
        }
        Section("Offline") {
            Info("Músicas no aparelho", "$offlineCount (%.1f MB)".format(offlineBytes.toDouble() / 1_048_576))
            TextButton(onClick = { nav.navigate("downloaded") }) { Text("Ver músicas baixadas") }
        }
        Section("App") {
            Info("Versão", Updater.currentVersion)
            OutlinedButton(onClick = {
                scope.launch {
                    runCatching { Updater.check() }
                        .onSuccess { if (it == null) onMessage("Você já está na versão mais recente") }
                        .onFailure { onMessage("Não foi possível verificar: ${it.message}") }
                }
            }, modifier = Modifier.fillMaxWidth()) { Text("Verificar atualizações") }
        }
        Section("") {
            Button(
                onClick = { confirmLogout = true },
                colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error),
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Sair deste aparelho") }
        }
    }
    if (confirmLogout) {
        AlertDialog(
            onDismissRequest = { confirmLogout = false },
            title = { Text("Sair deste aparelho?") },
            text = { Text("O acesso deste aparelho é revogado no servidor. As músicas baixadas continuam aqui até você entrar de novo.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmLogout = false
                    PlayerConnection.stop()
                    scope.launch { Core.logout() }
                }) { Text("Sair") }
            },
            dismissButton = { TextButton(onClick = { confirmLogout = false }) { Text("Cancelar") } },
        )
    }
}

@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Column(Modifier.padding(16.dp)) {
        if (title.isNotEmpty()) Text(title, color = IlmarfyGreen, fontWeight = FontWeight.Bold, modifier = Modifier.padding(bottom = 8.dp))
        content()
    }
    HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)
}

@Composable
private fun Info(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Text(label, color = Subtext, modifier = Modifier.weight(1f))
        Text(value)
    }
}
