package br.com.ilmar.ilmarfy.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Block
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.ThumbDown
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import br.com.ilmar.ilmarfy.core.Core
import br.com.ilmar.ilmarfy.core.friendly
import br.com.ilmar.ilmarfy.playback.PlaybackService
import br.com.ilmar.ilmarfy.playback.PlayerConnection
import kotlinx.coroutines.launch
import uniffi.ilmarfy_ffi.FlowRestricted
import uniffi.ilmarfy_ffi.FlowTrack

/** Ilmarfy Flow: músicas que você ainda não tem, de artistas da sua biblioteca e parecidos */
@Composable
fun FlowScreen(onMessage: (String) -> Unit) {
    var tracks by remember { mutableStateOf<List<FlowTrack>?>(null) }
    var seeds by remember { mutableStateOf<List<String>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    var restricted by remember { mutableStateOf<FlowRestricted?>(null) }
    var showRestricted by remember { mutableStateOf(false) }
    val now by PlayerConnection.state.collectAsState()
    val scope = rememberCoroutineScope()

    fun load() {
        tracks = null
        error = null
        scope.launch {
            runCatching { Core.io { it.flow() } }
                .onSuccess { tracks = it.tracks; seeds = it.seeds }
                .onFailure { error = it.friendly(); tracks = emptyList() }
            restricted = runCatching { Core.io { it.flowRestricted() } }.getOrNull()
        }
    }
    LaunchedEffect(Unit) { load() }

    fun dislike(t: FlowTrack, blockArtist: Boolean) = scope.launch {
        runCatching { Core.io { it.flowDislike(t, blockArtist) } }
            .onSuccess { r ->
                restricted = r
                tracks = tracks?.filterNot { it.videoId == t.videoId || (blockArtist && it.artist.equals(t.artist, true)) }
                onMessage(if (blockArtist) "🚫 ${t.artist} foi para os Restritos" else "👎 \"${t.title}\" foi para os Restritos")
                if (now.mediaId == PlaybackService.FLOW_PREFIX + t.videoId) PlayerConnection.next()
            }
            .onFailure { onMessage(it.friendly()) }
    }

    val list = tracks
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Column(Modifier.padding(16.dp)) {
                Text("Ilmarfy Flow", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Black, color = IlmarfyGreen)
                Text(
                    if (seeds.isEmpty()) "Músicas que você ainda não tem, a partir dos seus artistas."
                    else "Baseado em: ${seeds.joinToString()}",
                    color = Subtext,
                )
                Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { list?.let { PlayerConnection.playFlow(it) } }, enabled = !list.isNullOrEmpty()) {
                        Icon(Icons.Rounded.PlayArrow, null)
                        Text("Tocar Flow")
                    }
                    OutlinedButton(onClick = ::load) {
                        Icon(Icons.Rounded.Refresh, null)
                        Text("Atualizar")
                    }
                }
                val n = restricted?.let { it.blockedArtists.size + it.dislikedTracks.size } ?: 0
                TextButton(onClick = { showRestricted = true }, contentPadding = androidx.compose.foundation.layout.PaddingValues(0.dp)) {
                    Icon(Icons.Rounded.Block, null, tint = Subtext)
                    Text(if (n > 0) "  Restritos ($n)" else "  Restritos", color = Subtext)
                }
            }
        }
        when {
            list == null -> item {
                Row(Modifier.padding(32.dp), verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.width(24.dp), strokeWidth = 2.dp)
                    Text("  Sintonizando seu Flow…", color = Subtext)
                }
            }
            error != null -> item { Text(error!!, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(16.dp)) }
            list.isEmpty() -> item { Text("Nenhuma faixa inédita agora. Toque em Atualizar.", color = Subtext, modifier = Modifier.padding(16.dp)) }
            else -> items(list.size, key = { list[it].videoId }) { i ->
                val t = list[i]
                val playing = now.mediaId == PlaybackService.FLOW_PREFIX + t.videoId
                Row(
                    Modifier.fillMaxWidth().clickable { PlayerConnection.playFlow(list, i) }.padding(start = 16.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Cover(t.coverUrl, 48.dp, seed = t.artist)
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(t.title, fontWeight = FontWeight.SemiBold, color = if (playing) IlmarfyGreen else MaterialTheme.colorScheme.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(t.artist, color = Subtext, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    IconButton(onClick = {
                        scope.launch {
                            runCatching { Core.io { it.flowSave(t) } }
                                .onSuccess { onMessage("\"${t.title}\" vai ser baixada para a biblioteca") }
                                .onFailure { onMessage(it.friendly()) }
                        }
                    }) { Icon(Icons.Rounded.Add, "Adicionar à biblioteca", tint = IlmarfyGreen) }
                    IconButton(onClick = { dislike(t, false) }) { Icon(Icons.Rounded.ThumbDown, "Não curti", tint = Subtext) }
                    IconButton(onClick = { dislike(t, true) }) { Icon(Icons.Rounded.Block, "Bloquear artista", tint = Subtext) }
                }
            }
        }
    }

    if (showRestricted) {
        RestrictedDialog(restricted, onClose = { showRestricted = false }, onChanged = { restricted = it }, onMessage = onMessage)
    }
}

@Composable
private fun RestrictedDialog(data: FlowRestricted?, onClose: () -> Unit, onChanged: (FlowRestricted) -> Unit, onMessage: (String) -> Unit) {
    var tab by remember { mutableIntStateOf(0) }
    val scope = rememberCoroutineScope()
    fun restore(label: String, call: (uniffi.ilmarfy_ffi.IlmarfyCore) -> FlowRestricted) = scope.launch {
        runCatching { Core.io(call) }
            .onSuccess { onChanged(it); onMessage("↩️ $label restaurado no Flow") }
            .onFailure { onMessage(it.friendly()) }
    }
    AlertDialog(
        onDismissRequest = onClose,
        confirmButton = { TextButton(onClick = onClose) { Text("Fechar") } },
        title = { Text("Restritos do Flow") },
        text = {
            Column {
                PrimaryTabRow(selectedTabIndex = tab) {
                    Tab(tab == 0, { tab = 0 }, text = { Text("Músicas (${data?.dislikedTracks?.size ?: 0})") })
                    Tab(tab == 1, { tab = 1 }, text = { Text("Artistas (${data?.blockedArtists?.size ?: 0})") })
                }
                LazyColumn(Modifier.heightIn(max = 420.dp)) {
                    if (tab == 0) {
                        items(data?.dislikedTracks.orEmpty(), key = { it.videoId }) { t ->
                            RestrictedRow(t.title, t.artist) { restore(t.title) { it.flowRestoreTrack(t.videoId) } }
                        }
                    } else {
                        items(data?.blockedArtists.orEmpty(), key = { it.key }) { a ->
                            RestrictedRow(a.name, null) { restore(a.name) { it.flowUnblockArtist(a.key) } }
                        }
                    }
                }
            }
        },
    )
}

@Composable
private fun RestrictedRow(title: String, subtitle: String?, onRestore: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            subtitle?.let { Text(it, color = Subtext, style = MaterialTheme.typography.bodySmall, maxLines = 1) }
        }
        TextButton(onClick = onRestore) { Text("Restaurar") }
    }
}
