package br.com.ilmar.ilmarfy.ui

import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Home
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.Waves
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import br.com.ilmar.ilmarfy.core.Core
import br.com.ilmar.ilmarfy.core.Updater
import br.com.ilmar.ilmarfy.core.friendly
import br.com.ilmar.ilmarfy.playback.PlayerConnection
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.ilmarfy_ffi.IlmarfyException

private data class Tab(val route: String, val label: String, val icon: ImageVector)

private val tabs = listOf(
    Tab("home", "Início", Icons.Rounded.Home),
    Tab("search", "Buscar", Icons.Rounded.Search),
    Tab("library", "Biblioteca", Icons.Rounded.LibraryMusic),
    Tab("flow", "Flow", Icons.Rounded.Waves),
)

/** Sincroniza (e trata token revogado). Devolve a mensagem para mostrar, se houver. */
suspend fun syncNow(): String? =
    runCatching { Core.io { it.sync() } }.fold(
        onSuccess = { Core.changed(); null },
        onFailure = {
            if (it is IlmarfyException.Unauthorized) {
                Core.forceLogout()
                "Este aparelho foi desconectado. Entre de novo."
            } else {
                it.friendly()
            }
        },
    )

@Composable
fun MainScreen() {
    val nav = rememberNavController()
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val message: (String) -> Unit = { msg -> scope.launch { snackbar.showSnackbar(msg) } }

    LaunchedEffect(Unit) {
        PlayerConnection.connect(context)
        // Mostra o cache na hora e atualiza em segundo plano
        syncNow()?.let { if (Core.io { c -> c.trackCount() } == 0u) message(it) }
    }
    val downloadMsg by Downloads.messages.collectAsState()
    LaunchedEffect(downloadMsg) {
        downloadMsg?.let { message(it); Downloads.consumeMessage() }
    }
    // Procura versão nova do app ao abrir (falha silenciosa: sem internet, tenta na próxima)
    LaunchedEffect(Unit) { runCatching { Updater.check() } }
    UpdateDialog()

    val playerMsg by PlayerConnection.messages.collectAsState()
    LaunchedEffect(playerMsg) {
        playerMsg?.let { message(it); PlayerConnection.consumeMessage() }
    }

    val backStack by nav.currentBackStackEntryAsState()
    val route = backStack?.destination?.route

    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        bottomBar = {
            if (route != "nowplaying") {
                Column {
                    MiniPlayer(onOpen = { nav.navigate("nowplaying") })
                    NavigationBar(containerColor = MaterialTheme.colorScheme.surface) {
                        tabs.forEach { tab ->
                            NavigationBarItem(
                                selected = route == tab.route,
                                onClick = {
                                    nav.navigate(tab.route) {
                                        popUpTo("home") { saveState = true }
                                        launchSingleTop = true
                                        restoreState = true
                                    }
                                },
                                icon = { Icon(tab.icon, null) },
                                label = { Text(tab.label) },
                                colors = NavigationBarItemDefaults.colors(selectedIconColor = IlmarfyGreen, selectedTextColor = IlmarfyGreen),
                            )
                        }
                    }
                }
            }
        },
    ) { padding ->
        NavHost(nav, startDestination = "home", modifier = Modifier.padding(padding)) {
            composable("home") { HomeScreen(nav, message) }
            composable("search") { SearchScreen(message) }
            composable("library") { LibraryScreen(nav, message) }
            composable("flow") { FlowScreen(message) }
            composable("settings") { SettingsScreen(nav, message) }
            composable("nowplaying") { NowPlayingScreen(onClose = { nav.popBackStack() }, onMessage = message) }
            composable("artist/{name}") { e -> ArtistScreen(nav, Uri.decode(e.arguments?.getString("name").orEmpty()), message) }
            composable("playlist/{id}") { e -> PlaylistScreen(nav, e.arguments?.getString("id").orEmpty(), message) }
            composable("favorites") { FavoritesScreen(nav, message) }
            composable("downloaded") { DownloadedScreen(nav, message) }
        }
    }
}

fun NavHostController.openArtist(name: String) = navigate("artist/${Uri.encode(name)}")

@Composable
private fun MiniPlayer(onOpen: () -> Unit) {
    val now by PlayerConnection.state.collectAsState()
    if (now.mediaId == null) return
    var progress by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(now.isPlaying, now.mediaId) {
        while (true) {
            val s = PlayerConnection.state.value
            progress = if (s.durationMs > 0) PlayerConnection.position.toFloat() / s.durationMs else 0f
            if (!s.isPlaying) break
            delay(500)
        }
    }
    Column(
        Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .clickable(onClick = onOpen),
    ) {
        Row(Modifier.padding(horizontal = 12.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Cover(now.artworkUri?.toString(), 44.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(now.title, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    if (now.isFlow) "Flow • ${now.artist}" else now.artist,
                    color = Subtext,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            IconButton(onClick = PlayerConnection::togglePlay) {
                Icon(if (now.isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow, if (now.isPlaying) "Pausar" else "Tocar")
            }
            IconButton(onClick = { PlayerConnection.next() }, enabled = now.hasNext) { Icon(Icons.Rounded.SkipNext, "Próxima") }
        }
        LinearProgressIndicator(
            progress = { progress.coerceIn(0f, 1f) },
            modifier = Modifier.fillMaxWidth().height(2.dp),
            color = IlmarfyGreen,
            trackColor = MaterialTheme.colorScheme.surfaceVariant,
            drawStopIndicator = {},
        )
    }
}
