package br.com.ilmar.ilmarfy.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Block
import androidx.compose.material.icons.rounded.Lyrics
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.RepeatOne
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.media3.common.Player
import androidx.compose.ui.graphics.Color
import br.com.ilmar.ilmarfy.core.Core
import br.com.ilmar.ilmarfy.core.friendly
import br.com.ilmar.ilmarfy.playback.PlayerConnection
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.ilmarfy_ffi.Lyrics

private enum class Panel { Cover, Lyrics, Queue }

@Composable
fun NowPlayingScreen(onClose: () -> Unit, onMessage: (String) -> Unit) {
    val now by PlayerConnection.state.collectAsState()
    var position by remember { mutableLongStateOf(0L) }
    var dragging by remember { mutableStateOf<Float?>(null) }
    var panel by remember { mutableStateOf(Panel.Cover) }
    var favorite by remember { mutableStateOf(false) }
    var restricted by remember { mutableStateOf(false) }
    var lyrics by remember { mutableStateOf<Lyrics?>(null) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(Unit) {
        while (true) {
            position = PlayerConnection.position
            delay(250)
        }
    }
    LaunchedEffect(now.trackId) {
        val id = now.trackId
        favorite = id != null && runCatching { Core.io { it.isFavorite(id) } }.getOrDefault(false)
        restricted = id != null && runCatching { Core.io { it.track(id)?.restricted } }.getOrNull() == true
        lyrics = null
        if (id != null) lyrics = runCatching { Core.io { it.lyrics(id) } }.getOrNull()
    }
    if (now.mediaId == null) {
        LaunchedEffect(Unit) { onClose() }
        return
    }

    Column(Modifier.fillMaxSize().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onClose) { Icon(Icons.Rounded.KeyboardArrowDown, "Fechar") }
            Text(
                if (now.isFlow) "Tocando do Flow" else "Tocando agora",
                modifier = Modifier.weight(1f),
                textAlign = TextAlign.Center,
                color = Subtext,
                style = MaterialTheme.typography.labelLarge,
            )
            IconButton(onClick = { panel = if (panel == Panel.Lyrics) Panel.Cover else Panel.Lyrics }, enabled = now.trackId != null) {
                Icon(Icons.Rounded.Lyrics, "Letra", tint = if (panel == Panel.Lyrics) IlmarfyGreen else MaterialTheme.colorScheme.onSurface)
            }
            IconButton(onClick = { panel = if (panel == Panel.Queue) Panel.Cover else Panel.Queue }) {
                Icon(Icons.AutoMirrored.Rounded.QueueMusic, "Fila", tint = if (panel == Panel.Queue) IlmarfyGreen else MaterialTheme.colorScheme.onSurface)
            }
        }

        Column(Modifier.weight(1f).fillMaxWidth().padding(vertical = 16.dp), verticalArrangement = Arrangement.Center) {
            when (panel) {
                Panel.Cover -> Cover(now.artworkUri?.toString(), 400.dp, Modifier.fillMaxWidth().aspectRatio(1f), corner = 12.dp, seed = now.artist + now.album)
                Panel.Lyrics -> LyricsPanel(lyrics, position)
                Panel.Queue -> QueuePanel(now.mediaId)
            }
        }

        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(now.title, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(now.artist, color = Subtext, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            now.trackId?.let { id ->
                IconButton(onClick = {
                    scope.launch { setRestricted(id, now.title, !restricted, onMessage)?.let { restricted = it } }
                }) {
                    Icon(Icons.Rounded.Block, if (restricted) "Tirar das restritas" else "Restringir", tint = if (restricted) Color(0xFFFF5252) else Subtext)
                }
                IconButton(onClick = {
                    scope.launch {
                        runCatching { Core.io { it.toggleFavorite(id) } }
                            .onSuccess { favorite = it; Core.changed() }
                            .onFailure { onMessage(it.friendly()) }
                    }
                }) { FavoriteIcon(favorite) }
            }
        }

        val duration = now.durationMs.coerceAtLeast(1)
        Slider(
            value = dragging ?: (position.toFloat() / duration).coerceIn(0f, 1f),
            onValueChange = { dragging = it },
            onValueChangeFinished = {
                dragging?.let { PlayerConnection.seekTo((it * duration).toLong()) }
                dragging = null
            },
            colors = SliderDefaults.colors(thumbColor = IlmarfyGreen, activeTrackColor = IlmarfyGreen, inactiveTrackColor = MaterialTheme.colorScheme.surfaceVariant),
        )
        Row(Modifier.fillMaxWidth()) {
            Text(formatDuration(position / 1000), color = Subtext, style = MaterialTheme.typography.bodySmall)
            Spacer(Modifier.weight(1f))
            Text(formatDuration(now.durationMs / 1000), color = Subtext, style = MaterialTheme.typography.bodySmall)
        }

        Row(
            Modifier.fillMaxWidth().padding(vertical = 16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = PlayerConnection::toggleShuffle) {
                Icon(Icons.Rounded.Shuffle, "Aleatório", tint = if (now.shuffle) IlmarfyGreen else Subtext)
            }
            IconButton(onClick = { PlayerConnection.previous() }, modifier = Modifier.size(56.dp)) {
                Icon(Icons.Rounded.SkipPrevious, "Anterior", modifier = Modifier.size(36.dp))
            }
            FilledIconButton(
                onClick = PlayerConnection::togglePlay,
                modifier = Modifier.size(72.dp),
                colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.onSurface),
            ) {
                Icon(if (now.isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow, null, tint = MaterialTheme.colorScheme.background, modifier = Modifier.size(40.dp))
            }
            IconButton(onClick = { PlayerConnection.next() }, enabled = now.hasNext, modifier = Modifier.size(56.dp)) {
                Icon(Icons.Rounded.SkipNext, "Próxima", modifier = Modifier.size(36.dp))
            }
            IconButton(onClick = PlayerConnection::cycleRepeat) {
                Icon(
                    if (now.repeatMode == Player.REPEAT_MODE_ONE) Icons.Rounded.RepeatOne else Icons.Rounded.Repeat,
                    "Repetir",
                    tint = if (now.repeatMode == Player.REPEAT_MODE_OFF) Subtext else IlmarfyGreen,
                )
            }
        }
        Spacer(Modifier.height(16.dp))
    }
}

@Composable
private fun LyricsPanel(lyrics: Lyrics?, positionMs: Long) {
    if (lyrics == null) {
        Text("Sem letra para esta música.", color = Subtext, modifier = Modifier.fillMaxWidth(), textAlign = TextAlign.Center)
        return
    }
    if (lyrics.synced.isEmpty()) {
        LazyColumn { item { Text(lyrics.plain.ifBlank { "Instrumental" }, lineHeight = 26.sp) } }
        return
    }
    val current = lyrics.synced.indexOfLast { it.timeMs.toLong() <= positionMs }
    val listState = rememberLazyListState()
    LaunchedEffect(current) { if (current > 2) listState.animateScrollToItem(current - 2) }
    LazyColumn(state = listState) {
        items(lyrics.synced.size) { i ->
            Text(
                lyrics.synced[i].text.ifBlank { "♪" },
                fontSize = 20.sp,
                lineHeight = 28.sp,
                fontWeight = if (i == current) FontWeight.Bold else FontWeight.Normal,
                color = if (i == current) IlmarfyGreen else Subtext,
                modifier = Modifier.padding(vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun QueuePanel(currentId: String?) {
    val items = remember(currentId) { PlayerConnection.queue() }
    val listState = rememberLazyListState()
    val currentIndex = items.indexOfFirst { it.second.mediaId == currentId }
    LaunchedEffect(currentIndex) { if (currentIndex > 0) listState.scrollToItem(currentIndex) }
    LazyColumn(state = listState) {
        items(items.size) { i ->
            val (index, item) = items[i]
            val md = item.mediaMetadata
            Row(
                Modifier.fillMaxWidth().clickable { PlayerConnection.jumpTo(index) }.padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Cover(md.artworkUri?.toString(), 40.dp, corner = 4.dp, seed = md.artist.toString() + md.albumTitle.toString())
                Spacer(Modifier.size(12.dp))
                Column {
                    Text(
                        md.title?.toString().orEmpty(),
                        color = if (item.mediaId == currentId) IlmarfyGreen else MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(md.artist?.toString().orEmpty(), color = Subtext, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                }
            }
        }
    }
}

