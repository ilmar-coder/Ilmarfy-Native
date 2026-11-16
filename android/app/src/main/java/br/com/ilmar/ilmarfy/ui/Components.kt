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
import androidx.compose.material.icons.rounded.Block
import androidx.compose.material.icons.rounded.Restore
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
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

/** Cor viva derivada do texto (mesmo artista/álbum => mesma cor), para capas geradas */
fun seedColor(seed: String): Color {
    val hue = ((seed.hashCode() % 360) + 360) % 360
    return Color.hsv(hue.toFloat(), 0.55f, 0.62f)
}

private fun Color.darker(f: Float = 0.35f) = Color(red * f, green * f, blue * f, alpha)

/**
 * Capa da música. Sem imagem (ou se ela não carregar) aparece uma capa gerada com a cor do
 * artista/álbum e uma nota musical, como o Spotify faz.
 */
@Composable
fun Cover(url: String?, size: Dp, modifier: Modifier = Modifier, corner: Dp = 6.dp, seed: String = "") {
    val base = seedColor(seed.ifBlank { url.orEmpty() })
    Box(
        modifier
            .size(size)
            .clip(RoundedCornerShape(corner))
            .background(Brush.verticalGradient(listOf(base, base.darker()))),
        contentAlignment = Alignment.Center,
    ) {
        Icon(Icons.Rounded.MusicNote, null, tint = Color.White.copy(alpha = 0.8f), modifier = Modifier.size(size / 2.5f))
        if (!url.isNullOrBlank()) {
            AsyncImage(model = url, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        }
    }
}

/** Capa de playlist: mosaico 2x2 com as capas das primeiras músicas (ou uma só, se tiver menos de 4) */
@Composable
fun PlaylistCover(covers: List<String>, size: Dp, seed: String, modifier: Modifier = Modifier, corner: Dp = 6.dp) {
    val distinct = covers.filter { it.isNotBlank() }.distinct().take(4)
    if (distinct.size < 4) {
        Cover(distinct.firstOrNull(), size, modifier, corner, seed)
        return
    }
    Column(modifier.size(size).clip(RoundedCornerShape(corner))) {
        for (row in 0..1) {
            Row(Modifier.weight(1f)) {
                for (col in 0..1) {
                    AsyncImage(
                        model = distinct[row * 2 + col],
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.weight(1f).fillMaxSize(),
                    )
                }
            }
        }
    }
}

/** Capa especial (Músicas curtidas, Offline): gradiente com um ícone */
@Composable
fun SpecialCover(icon: androidx.compose.ui.graphics.vector.ImageVector, top: Color, bottom: Color, size: Dp, corner: Dp = 6.dp) {
    Box(
        Modifier.size(size).clip(RoundedCornerShape(corner)).background(Brush.linearGradient(listOf(top, bottom))),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, tint = Color.White, modifier = Modifier.size(size / 2.3f))
    }
}

val LikedTop = Color(0xFF450AF5)
val LikedBottom = Color(0xFFC4EFD9)
val RankingTop = Color(0xFFE95F2A)
val RankingBottom = Color(0xFF5A143C)
val RestrictedTop = Color(0xFF8A1C2B)
val RestrictedBottom = Color(0xFF2A0A10)

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
    rank: Int? = null,
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
        if (rank != null) {
            Text(
                rank.toString(),
                modifier = Modifier.width(32.dp),
                color = if (rank <= 3) IlmarfyGreen else Subtext,
                fontWeight = if (rank <= 3) FontWeight.Black else FontWeight.Normal,
                style = MaterialTheme.typography.titleMedium,
                textAlign = TextAlign.Center,
            )
            Spacer(Modifier.width(4.dp))
        }
        Cover(track.coverUrl, 48.dp, seed = track.artist + track.album)
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
                    if (rank != null) "${track.artist} • ${formatPlays(track.playCount)}" else "${track.artist} • ${formatDuration(track.durationSecs)}",
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
                DropdownMenuItem(
                    text = { Text(if (track.restricted) "Tirar das restritas" else "Restringir") },
                    leadingIcon = { Icon(if (track.restricted) Icons.Rounded.Restore else Icons.Rounded.Block, null) },
                    onClick = {
                        menu = false
                        scope.launch { setRestricted(track, !track.restricted, onMessage) }
                    },
                )
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
    ranking: Boolean = false,
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
                rank = if (ranking) i + 1 else null,
            )
        }
        item { Spacer(Modifier.size(16.dp)) }
    }
}

@Composable
fun FavoriteIcon(on: Boolean) =
    Icon(if (on) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder, "Favorita", tint = if (on) IlmarfyGreen else Subtext)

/** Manda a faixa para "Restritas" (some da biblioteca) ou a devolve. Devolve o novo estado, ou null se falhou. */
suspend fun setRestricted(track: Track, on: Boolean, onMessage: (String) -> Unit): Boolean? = setRestricted(track.id, track.title, on, onMessage)

suspend fun setRestricted(id: String, title: String, on: Boolean, onMessage: (String) -> Unit): Boolean? =
    runCatching { Core.io { it.setRestricted(id, on) } }
        .onSuccess {
            onMessage(if (on) "\"$title\" foi para Restritas" else "\"$title\" voltou para a biblioteca")
            Core.changed()
        }
        .onFailure { onMessage(it.friendly()) }
        .map { on }
        .getOrNull()

/** 1234 -> "1.234 reproduções" */
fun formatPlays(n: Long): String = "%,d".format(java.util.Locale.forLanguageTag("pt-BR"), n) + if (n == 1L) " reprodução" else " reproduções"
