package br.com.ilmar.ilmarfy.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.DownloadDone
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import br.com.ilmar.ilmarfy.core.Core
import br.com.ilmar.ilmarfy.core.friendly
import br.com.ilmar.ilmarfy.playback.PlayerConnection
import kotlinx.coroutines.launch
import uniffi.ilmarfy_ffi.Album
import uniffi.ilmarfy_ffi.Artist
import uniffi.ilmarfy_ffi.Playlist
import uniffi.ilmarfy_ffi.Track

@Composable
fun LibraryScreen(nav: NavHostController, onMessage: (String) -> Unit) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    Column(Modifier.fillMaxSize()) {
        Text("Biblioteca", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Black, modifier = Modifier.padding(16.dp))
        PrimaryTabRow(selectedTabIndex = tab) {
            listOf("Playlists", "Artistas", "Offline").forEachIndexed { i, label ->
                Tab(selected = tab == i, onClick = { tab = i }, text = { Text(label) })
            }
        }
        when (tab) {
            0 -> PlaylistsTab(nav, onMessage)
            1 -> ArtistsTab(nav)
            else -> DownloadedTab(onMessage)
        }
    }
}

@Composable
private fun PlaylistsTab(nav: NavHostController, onMessage: (String) -> Unit) {
    val version by Core.version.collectAsState()
    var playlists by remember { mutableStateOf<List<Playlist>>(emptyList()) }
    var creating by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(version) { playlists = runCatching { Core.io { it.playlists() } }.getOrDefault(emptyList()) }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            ListRow(title = "Nova playlist", subtitle = null, icon = { Icon(Icons.Rounded.Add, null, tint = IlmarfyGreen) }) { creating = true }
        }
        items(playlists, key = { it.id }) { p ->
            val fav = p.name.equals("Favoritos 2026", ignoreCase = true)
            ListRow(
                title = p.name,
                subtitle = "${p.trackIds.size} faixas",
                icon = { if (fav) Icon(Icons.Rounded.Favorite, null, tint = IlmarfyGreen) else Cover(null, 48.dp) },
            ) { nav.navigate("playlist/${p.id}") }
        }
    }
    if (creating) {
        var name by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { creating = false },
            title = { Text("Nova playlist") },
            text = { OutlinedTextField(name, { name = it }, singleLine = true, label = { Text("Nome") }) },
            confirmButton = {
                TextButton(enabled = name.isNotBlank(), onClick = {
                    creating = false
                    scope.launch {
                        runCatching { Core.io { it.createPlaylist(name.trim()) } }
                            .onSuccess { Core.changed() }
                            .onFailure { onMessage(it.friendly()) }
                    }
                }) { Text("Criar") }
            },
            dismissButton = { TextButton(onClick = { creating = false }) { Text("Cancelar") } },
        )
    }
}

@Composable
private fun ArtistsTab(nav: NavHostController) {
    val version by Core.version.collectAsState()
    var artists by remember { mutableStateOf<List<Artist>>(emptyList()) }
    LaunchedEffect(version) { artists = runCatching { Core.io { it.artists() } }.getOrDefault(emptyList()) }
    LazyColumn(Modifier.fillMaxSize()) {
        items(artists, key = { it.name }) { a ->
            ListRow(a.name, "${a.trackCount} faixas", icon = { Cover(a.coverUrl, 48.dp, corner = 24.dp) }) { nav.openArtist(a.name) }
        }
    }
}

@Composable
private fun DownloadedTab(onMessage: (String) -> Unit) {
    val version by Core.version.collectAsState()
    var tracks by remember { mutableStateOf<List<Track>>(emptyList()) }
    var size by remember { mutableStateOf(0uL) }
    LaunchedEffect(version) {
        runCatching {
            Core.io { c ->
                size = c.downloadsSize()
                c.downloadedIds().mapNotNull { c.track(it) }.sortedBy { it.title.lowercase() }
            }
        }.onSuccess { tracks = it }
    }
    TrackList(tracks, onMessage, header = {
        Text(
            if (tracks.isEmpty()) "Nada baixado ainda. Abra uma playlist e toque em \"Baixar\" para ouvir sem internet."
            else "${tracks.size} faixas • %.1f MB no aparelho".format(size.toDouble() / 1_048_576),
            color = Subtext,
            modifier = Modifier.padding(16.dp),
        )
    })
}

@Composable
fun ListRow(title: String, subtitle: String?, icon: @Composable () -> Unit, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        androidx.compose.foundation.layout.Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) { icon() }
        Spacer(Modifier.width(12.dp))
        Column {
            Text(title, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            subtitle?.let { Text(it, color = Subtext, style = MaterialTheme.typography.bodySmall) }
        }
    }
}

/** Cabeçalho comum: voltar, título, tocar/aleatório */
@Composable
fun DetailHeader(
    nav: NavHostController,
    title: String,
    subtitle: String,
    tracks: List<Track>,
    actions: @Composable () -> Unit = {},
) {
    Column(Modifier.padding(bottom = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = { nav.popBackStack() }) { Icon(Icons.AutoMirrored.Rounded.ArrowBack, "Voltar") }
            Spacer(Modifier.weight(1f))
            actions()
        }
        Text(title, style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Black, modifier = Modifier.padding(horizontal = 16.dp))
        Text(subtitle, color = Subtext, modifier = Modifier.padding(horizontal = 16.dp))
        Row(Modifier.padding(16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { PlayerConnection.play(tracks) }, enabled = tracks.isNotEmpty()) {
                Icon(Icons.Rounded.PlayArrow, null)
                Text("Tocar")
            }
            FilledTonalButton(onClick = { PlayerConnection.play(tracks, shuffle = true) }, enabled = tracks.isNotEmpty()) {
                Icon(Icons.Rounded.Shuffle, null)
                Text("Aleatório")
            }
        }
    }
}

@Composable
fun PlaylistScreen(nav: NavHostController, id: String, onMessage: (String) -> Unit) {
    val version by Core.version.collectAsState()
    val active by Downloads.active.collectAsState()
    var playlist by remember { mutableStateOf<Playlist?>(null) }
    var tracks by remember { mutableStateOf<List<Track>>(emptyList()) }
    var offlineCount by remember { mutableIntStateOf(0) }
    var confirmDelete by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(version, id) {
        runCatching {
            Core.io { c ->
                playlist = c.playlists().find { it.id == id }
                tracks = c.playlistTracks(id)
                val ids = c.downloadedIds().toSet()
                offlineCount = tracks.count { it.id in ids }
            }
        }
    }
    val progress = active["playlist:$id"]
    Column(Modifier.fillMaxSize()) {
        DetailHeader(
            nav,
            playlist?.name ?: "",
            "${tracks.size} faixas • $offlineCount offline",
            tracks,
            actions = {
                IconButton(onClick = { confirmDelete = true }) { Icon(Icons.Rounded.Delete, "Apagar playlist") }
            },
        )
        when {
            progress != null -> Column(Modifier.padding(horizontal = 16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Baixando: ${progress.label}", style = MaterialTheme.typography.bodySmall, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    TextButton(onClick = { Downloads.cancel("playlist:$id") }) { Text("Cancelar") }
                }
                LinearProgressIndicator(progress = { progress.fraction ?: 0f }, modifier = Modifier.fillMaxWidth(), color = IlmarfyGreen)
            }
            tracks.isNotEmpty() && offlineCount == tracks.size -> Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Rounded.DownloadDone, null, tint = IlmarfyGreen)
                Text(" Disponível offline", color = IlmarfyGreen)
            }
            tracks.isNotEmpty() -> TextButton(onClick = { Downloads.downloadPlaylist(id, playlist?.name ?: "") }, modifier = Modifier.padding(horizontal = 8.dp)) {
                Icon(Icons.Rounded.Download, null)
                Text(" Baixar para ouvir offline")
            }
        }
        TrackList(tracks, onMessage)
    }
    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("Apagar \"${playlist?.name}\"?") },
            text = { Text("A playlist some do Ilmarfy (web e apps). As músicas continuam na biblioteca.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    scope.launch {
                        runCatching { Core.io { it.deletePlaylist(id) } }
                            .onSuccess { Core.changed(); nav.popBackStack() }
                            .onFailure { onMessage(it.friendly()) }
                    }
                }) { Text("Apagar") }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text("Cancelar") } },
        )
    }
}

@Composable
fun FavoritesScreen(nav: NavHostController, onMessage: (String) -> Unit) {
    val version by Core.version.collectAsState()
    var tracks by remember { mutableStateOf<List<Track>>(emptyList()) }
    LaunchedEffect(version) { tracks = runCatching { Core.io { it.favorites() } }.getOrDefault(emptyList()) }
    Column(Modifier.fillMaxSize()) {
        DetailHeader(nav, "Favoritos", "${tracks.size} faixas", tracks)
        TrackList(tracks, onMessage)
    }
}

@Composable
fun DownloadedScreen(nav: NavHostController, onMessage: (String) -> Unit) {
    Column(Modifier.fillMaxSize()) {
        DetailHeader(nav, "Offline", "Músicas no aparelho", emptyList())
        DownloadedTab(onMessage)
    }
}

@Composable
fun ArtistScreen(nav: NavHostController, name: String, onMessage: (String) -> Unit) {
    val version by Core.version.collectAsState()
    var tracks by remember { mutableStateOf<List<Track>>(emptyList()) }
    var albums by remember { mutableStateOf<List<Album>>(emptyList()) }
    LaunchedEffect(version, name) {
        runCatching {
            Core.io { c ->
                tracks = c.artistTracks(name)
                albums = c.albums(name)
            }
        }
    }
    Column(Modifier.fillMaxSize()) {
        DetailHeader(nav, name, "${tracks.size} faixas • ${albums.size} álbuns", tracks)
        TrackList(tracks, onMessage)
    }
}
