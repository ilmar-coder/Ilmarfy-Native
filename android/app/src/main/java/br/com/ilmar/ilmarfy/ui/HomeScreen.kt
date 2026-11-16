package br.com.ilmar.ilmarfy.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.AssistChip
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import br.com.ilmar.ilmarfy.core.Core
import br.com.ilmar.ilmarfy.playback.PlayerConnection
import kotlinx.coroutines.launch
import uniffi.ilmarfy_ffi.Playlist
import uniffi.ilmarfy_ffi.Track
import uniffi.ilmarfy_ffi.TrackOrder
import java.util.Calendar

private data class HomeData(val count: UInt, val top: List<Track>, val recent: List<Track>, val all: List<Track>, val playlists: List<Playlist>, val covers: Map<String, List<String>>)

private fun greeting(): String = when (Calendar.getInstance().get(Calendar.HOUR_OF_DAY)) {
    in 5..11 -> "Bom dia"
    in 12..17 -> "Boa tarde"
    else -> "Boa noite"
}

@Composable
fun HomeScreen(nav: NavHostController, onMessage: (String) -> Unit) {
    val version by Core.version.collectAsState()
    var data by remember { mutableStateOf<HomeData?>(null) }
    var syncing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(version) {
        data = runCatching {
            Core.io {
                val playlists = it.playlists()
                HomeData(
                    it.trackCount(),
                    it.tracks(TrackOrder.MOST_PLAYED).take(20),
                    it.tracks(TrackOrder.RECENTLY_ADDED).take(20),
                    it.tracks(TrackOrder.TITLE),
                    playlists,
                    playlists.associate { p -> p.id to it.playlistTracks(p.id).map { t -> t.coverUrl }.take(12) },
                )
            }
        }.getOrNull()
    }

    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 16.dp)) {
        item {
            Row(Modifier.fillMaxWidth().padding(start = 16.dp, top = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(greeting(), style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Black, modifier = Modifier.weight(1f))
                IconButton(onClick = {
                    syncing = true
                    scope.launch {
                        onMessage(syncNow() ?: "Biblioteca atualizada")
                        syncing = false
                    }
                }) {
                    if (syncing) CircularProgressIndicator(Modifier.width(20.dp), strokeWidth = 2.dp) else Icon(Icons.Rounded.Refresh, "Sincronizar")
                }
                IconButton(onClick = { nav.navigate("settings") }) { Icon(Icons.Rounded.Settings, "Configurações") }
            }
        }
        val d = data
        if (d == null || d.count == 0u) {
            item {
                Text(
                    if (d == null) "Carregando…" else "Biblioteca vazia. Toque em sincronizar para buscar suas músicas.",
                    color = Subtext,
                    modifier = Modifier.padding(16.dp),
                )
            }
            return@LazyColumn
        }
        item {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                AssistChip(onClick = { PlayerConnection.play(d.all, shuffle = true) }, label = { Text("Aleatório: ${d.count} músicas") }, leadingIcon = { Icon(Icons.Rounded.Shuffle, null) })
                AssistChip(onClick = { nav.navigate("favorites") }, label = { Text("Favoritos") })
            }
        }
        if (d.playlists.isNotEmpty()) {
            item { SectionTitle("Suas playlists") }
            item {
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    items(d.playlists, key = { it.id }) { p ->
                        Column(Modifier.width(120.dp).clickable { nav.navigate("playlist/${p.id}") }) {
                            if (p.name.equals("Favoritos 2026", ignoreCase = true)) {
                                SpecialCover(Icons.Rounded.Favorite, LikedTop, LikedBottom, 120.dp, corner = 8.dp)
                            } else {
                                PlaylistCover(d.covers[p.id].orEmpty(), 120.dp, p.name, corner = 8.dp)
                            }
                            Text(p.name, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text("${p.trackIds.size} faixas", color = Subtext, style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }
            }
        }
        item { SectionTitle("Mais tocadas", action = "Ver ranking") { nav.navigate("ranking") } }
        item { TrackCarousel(d.top) }
        item { SectionTitle("Adicionadas recentemente") }
        item { TrackCarousel(d.recent) }
    }
}

@Composable
fun SectionTitle(text: String, action: String? = null, onClick: (() -> Unit)? = null) {
    Row(
        Modifier.fillMaxWidth().then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(start = 16.dp, end = 16.dp, top = 20.dp, bottom = 8.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        Text(text, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
        if (action != null) Text(action, color = Subtext, style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun TrackCarousel(tracks: List<Track>) {
    LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        items(tracks.size, key = { tracks[it].id }) { i ->
            val t = tracks[i]
            Column(Modifier.width(130.dp).clickable { PlayerConnection.play(tracks, i) }) {
                Cover(t.coverUrl, 130.dp, corner = 8.dp, seed = t.artist + t.album)
                Spacer(Modifier.height(4.dp))
                Text(t.title, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(t.artist, color = Subtext, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}
