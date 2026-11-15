package br.com.ilmar.ilmarfy.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.DownloadDone
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import br.com.ilmar.ilmarfy.core.Core
import br.com.ilmar.ilmarfy.core.friendly
import br.com.ilmar.ilmarfy.playback.PlayerConnection
import coil3.compose.AsyncImage
import kotlinx.coroutines.launch
import uniffi.ilmarfy_ffi.Playlist
import uniffi.ilmarfy_ffi.Track

@Composable
fun Cover(url: String?, size: Dp, modifier: Modifier = Modifier, corner: Dp = 6.dp) {
    Box(
        modifier
            .size(size)
            .clip(RoundedCornerShape(corner))
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        Icon(Icons.Rounded.MusicNote, null, tint = Subtext, modifier = Modifier.size(size / 2.5f))
        if (!url.isNullOrBlank()) {
            AsyncImage(model = url, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        }
    }
}

fun formatDuration(secs: Long): String = "%d:%02d".format(secs / 60, secs % 60)

/** Linha de faixa: toque toca a lista a partir dela; menu com fila, favorito, playlist e offline */
@Composable
fun TrackRow(
    track: Track,
    downloaded: Boolean,
    playing: Boolean,
    onClick: () -> Unit,
    onMessage: (String) -> Unit,
    extraMenu: (@Composable (close: () -> Unit) -> Unit)? = null,
) {
    var menu by remember { mutableStateOf(false) }
    var pickPlaylist by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(track.coverUrl, 48.dp)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(
                track.title,
                color = if (playing) IlmarfyGreen else MaterialTheme.colorScheme.onSurface,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (downloaded) {
                    Icon(Icons.Rounded.DownloadDone, "Baixada", tint = IlmarfyGreen, modifier = Modifier.size(14.dp))
                    Spacer(Modifier.width(4.dp))
                }
                Text(
                    "${track.artist} • ${formatDuration(track.durationSecs)}",
                    color = Subtext,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Box {
            IconButton(onClick = { menu = true }) { Icon(Icons.Rounded.MoreVert, "Opções", tint = Subtext) }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(
                    text = { Text("Tocar a seguir") },
                    leadingIcon = { Icon(Icons.Rounded.SkipNext, null) },
                    onClick = { menu = false; PlayerConnection.playNext(track); onMessage("Toca a seguir") },
                )
                DropdownMenuItem(
                    text = { Text("Adicionar à fila") },
                    leadingIcon = { Icon(Icons.AutoMirrored.Rounded.QueueMusic, null) },
                    onClick = { menu = false; PlayerConnection.enqueue(track); onMessage("Adicionada à fila") },
                )
                DropdownMenuItem(
                    text = { Text("Favoritar / desfavoritar") },
                    leadingIcon = { Icon(Icons.Rounded.FavoriteBorder, null) },
                    onClick = {
                        menu = false
                        scope.launch {
                            runCatching { Core.io { it.toggleFavorite(track.id) } }
                                .onSuccess { onMessage(if (it) "Adicionada aos favoritos" else "Removida dos favoritos"); Core.changed() }
                                .onFailure { onMessage(it.friendly()) }
                        }
                    },
                )
                DropdownMenuItem(
                    text = { Text("Adicionar à playlist") },
                    leadingIcon = { Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, null) },
                    onClick = { menu = false; pickPlaylist = true },
                )
                if (downloaded) {
                    DropdownMenuItem(
                        text = { Text("Remover do aparelho") },
                        leadingIcon = { Icon(Icons.Rounded.Delete, null) },
                        onClick = { menu = false; Downloads.removeTrack(track.id) },
                    )
                } else {
                    DropdownMenuItem(
                        text = { Text("Baixar para ouvir offline") },
                        leadingIcon = { Icon(Icons.Rounded.Download, null) },
                        onClick = { menu = false; Downloads.downloadTrack(track.id, track.title) },
                    )
                }
                extraMenu?.invoke { menu = false }
            }
        }
    }
    if (pickPlaylist) {
        PlaylistPicker(
            onDismiss = { pickPlaylist = false },
            onPick = { p ->
                pickPlaylist = false
                scope.launch {
                    runCatching { Core.io { it.addToPlaylist(p.id, track.id) } }
                        .onSuccess { onMessage("Adicionada a \"${p.name}\""); Core.changed() }
                        .onFailure { onMessage(it.friendly()) }
                }
            },
        )
    }
}

@Composable
private fun PlaylistPicker(onDismiss: () -> Unit, onPick: (Playlist) -> Unit) {
    var playlists by remember { mutableStateOf<List<Playlist>>(emptyList()) }
    LaunchedEffect(Unit) { playlists = runCatching { Core.io { it.playlists() } }.getOrDefault(emptyList()) }
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onDismiss) { Text("Cancelar") } },
        title = { Text("Adicionar à playlist") },
        text = {
            LazyColumn {
                items(playlists, key = { it.id }) { p ->
                    Text(
                        p.name,
                        Modifier
                            .fillMaxWidth()
                            .clickable { onPick(p) }
                            .padding(vertical = 12.dp),
                    )
                }
            }
        },
    )
}

/** Lista de faixas com estado de "tocando" e "baixada" */
@Composable
fun TrackList(
    tracks: List<Track>,
    onMessage: (String) -> Unit,
    modifier: Modifier = Modifier,
    header: (@Composable () -> Unit)? = null,
) {
    val now by PlayerConnection.state.collectAsState()
    val version by Core.version.collectAsState()
    var downloaded by remember { mutableStateOf<Set<String>>(emptySet()) }
    LaunchedEffect(version) { downloaded = runCatching { Core.io { it.downloadedIds() }.toSet() }.getOrDefault(emptySet()) }
    LazyColumn(modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        if (header != null) item { header() }
        items(tracks.size, key = { tracks[it].id }) { i ->
            val t = tracks[i]
            TrackRow(
                track = t,
                downloaded = t.id in downloaded,
                playing = now.trackId == t.id,
                onClick = { PlayerConnection.play(tracks, i) },
                onMessage = onMessage,
            )
        }
        item { Spacer(Modifier.size(16.dp)) }
    }
}

@Composable
fun FavoriteIcon(on: Boolean) =
    Icon(if (on) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder, "Favorita", tint = if (on) IlmarfyGreen else Subtext)
