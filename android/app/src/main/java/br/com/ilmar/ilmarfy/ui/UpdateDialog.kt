package br.com.ilmar.ilmarfy.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import br.com.ilmar.ilmarfy.core.UpdateState
import br.com.ilmar.ilmarfy.core.Updater

/** Aviso de versão nova, progresso do download e erros da atualização */
@Composable
fun UpdateDialog() {
    val state by Updater.state.collectAsState()
    val context = LocalContext.current
    when (val s = state) {
        is UpdateState.Available -> AlertDialog(
            onDismissRequest = Updater::dismiss,
            title = { Text("Nova versão: ${s.release.versionName}") },
            text = {
                Column {
                    Text("Você está na ${Updater.currentVersion}.")
                    if (s.release.notes.isNotBlank()) Text(s.release.notes, modifier = Modifier.padding(top = 8.dp))
                }
            },
            confirmButton = { TextButton(onClick = { Updater.downloadAndInstall(context, s.release) }) { Text("Atualizar") } },
            dismissButton = { TextButton(onClick = Updater::dismiss) { Text("Depois") } },
        )
        is UpdateState.Downloading -> AlertDialog(
            onDismissRequest = {},
            title = { Text("Baixando ${s.release.versionName}…") },
            text = { LinearProgressIndicator(progress = { s.fraction }, modifier = Modifier.fillMaxWidth(), color = IlmarfyGreen) },
            confirmButton = {},
        )
        is UpdateState.Failed -> AlertDialog(
            onDismissRequest = Updater::dismiss,
            title = { Text("Atualização") },
            text = { Text(s.message) },
            confirmButton = { TextButton(onClick = Updater::dismiss) { Text("OK") } },
        )
        UpdateState.Idle -> Unit
    }
}
