package br.com.ilmar.ilmarfy.playback

import android.content.ComponentName
import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import androidx.media3.common.PlaybackException
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.guava.await
import br.com.ilmar.ilmarfy.core.Core
import uniffi.ilmarfy_ffi.FlowTrack
import uniffi.ilmarfy_ffi.Track

/** O que está tocando, para as telas */
data class NowPlaying(
    val mediaId: String? = null,
    val title: String = "",
    val artist: String = "",
    val album: String = "",
    val artworkUri: Uri? = null,
    val isPlaying: Boolean = false,
    val isBuffering: Boolean = false,
    val durationMs: Long = 0,
    val shuffle: Boolean = false,
    val repeatMode: Int = Player.REPEAT_MODE_OFF,
    val hasNext: Boolean = false,
    val hasPrevious: Boolean = false,
) {
    /** Id da faixa da biblioteca (null para prévias do Flow) */
    val trackId: String? get() = mediaId?.takeIf { it.startsWith(PlaybackService.TRACK_PREFIX) }?.removePrefix(PlaybackService.TRACK_PREFIX)
    val isFlow: Boolean get() = mediaId?.startsWith(PlaybackService.FLOW_PREFIX) == true
}

/** Liga a interface ao [PlaybackService] por um MediaController */
object PlayerConnection {
    private var controller: MediaController? = null
    private var appContext: Context? = null
    private val _state = MutableStateFlow(NowPlaying())
    val state: StateFlow<NowPlaying> = _state.asStateFlow()

    /** Avisos para a interface (ex.: sem internet, faixa que não carregou) */
    private val _messages = MutableStateFlow<String?>(null)
    val messages: StateFlow<String?> = _messages.asStateFlow()
    fun consumeMessage() {
        _messages.value = null
    }

    fun isOnline(): Boolean {
        val cm = appContext?.getSystemService(ConnectivityManager::class.java) ?: return true
        val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return false
        return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
    }

    val position: Long get() = controller?.currentPosition ?: 0

    suspend fun connect(context: Context) {
        if (controller != null) return
        appContext = context.applicationContext
        val token = SessionToken(context, ComponentName(context, PlaybackService::class.java))
        val c = MediaController.Builder(context, token).buildAsync().await()
        c.addListener(object : Player.Listener {
            override fun onEvents(player: Player, events: Player.Events) = refresh()

            override fun onPlayerError(error: PlaybackException) {
                _messages.value = if (!isOnline()) "Sem internet: essa música não está baixada" else "Não foi possível tocar esta música"
            }
        })
        controller = c
        refresh()
    }

    private fun refresh() {
        val c = controller ?: return
        val md = c.mediaMetadata
        _state.value = NowPlaying(
            mediaId = c.currentMediaItem?.mediaId,
            title = md.title?.toString().orEmpty(),
            artist = md.artist?.toString().orEmpty(),
            album = md.albumTitle?.toString().orEmpty(),
            artworkUri = md.artworkUri,
            isPlaying = c.isPlaying,
            isBuffering = c.playbackState == Player.STATE_BUFFERING,
            durationMs = c.duration.coerceAtLeast(0),
            shuffle = c.shuffleModeEnabled,
            repeatMode = c.repeatMode,
            hasNext = c.hasNextMediaItem(),
            hasPrevious = c.hasPreviousMediaItem(),
        )
    }

    private fun metadata(title: String, artist: String, album: String, cover: String) =
        MediaMetadata.Builder()
            .setTitle(title)
            .setArtist(artist)
            .setAlbumTitle(album)
            .setArtworkUri(cover.takeIf { it.isNotBlank() }?.let(Uri::parse))
            .build()

    fun itemOf(t: Track): MediaItem = MediaItem.Builder()
        .setMediaId(PlaybackService.TRACK_PREFIX + t.id)
        .setMediaMetadata(metadata(t.title, t.artist, t.album, t.coverUrl))
        .build()

    fun itemOf(t: FlowTrack): MediaItem = MediaItem.Builder()
        .setMediaId(PlaybackService.FLOW_PREFIX + t.videoId)
        .setMediaMetadata(metadata(t.title, t.artist, t.album, t.coverUrl))
        .build()

    /**
     * Toca a lista a partir de `start`; `shuffle` liga o aleatório.
     * Sem internet, a fila fica só com as músicas baixadas.
     */
    fun play(tracks: List<Track>, start: Int = 0, shuffle: Boolean = false) {
        if (tracks.isEmpty()) return
        if (isOnline()) return playItems(tracks.map(::itemOf), start, shuffle)

        val downloaded = runCatching { Core.get().downloadedIds().toSet() }.getOrDefault(emptySet())
        val chosen = tracks.getOrNull(start)
        if (!shuffle && chosen != null && chosen.id !in downloaded) {
            _messages.value = "Sem internet: \"${chosen.title}\" não está baixada"
            return
        }
        val offline = tracks.filter { it.id in downloaded }
        if (offline.isEmpty()) {
            _messages.value = "Sem internet e nenhuma dessas músicas está baixada"
            return
        }
        val newStart = chosen?.let { c -> offline.indexOfFirst { it.id == c.id } }?.takeIf { it >= 0 } ?: 0
        playItems(offline.map(::itemOf), newStart, shuffle)
        if (offline.size < tracks.size) _messages.value = "Sem internet: tocando só as ${offline.size} músicas baixadas"
    }

    fun playFlow(tracks: List<FlowTrack>, start: Int = 0) = playItems(tracks.map(::itemOf), start, false)

    private fun playItems(items: List<MediaItem>, start: Int, shuffle: Boolean) {
        val c = controller ?: return
        if (items.isEmpty()) return
        c.shuffleModeEnabled = shuffle
        val first = if (shuffle && start == 0) items.indices.random() else start.coerceIn(items.indices)
        c.setMediaItems(items, first, 0)
        c.prepare()
        c.play()
    }

    fun playNext(t: Track) {
        val c = controller ?: return
        if (c.mediaItemCount == 0) return play(listOf(t))
        c.addMediaItem(c.currentMediaItemIndex + 1, itemOf(t))
    }

    fun enqueue(t: Track) {
        val c = controller ?: return
        if (c.mediaItemCount == 0) return play(listOf(t))
        c.addMediaItem(itemOf(t))
    }

    fun togglePlay() {
        val c = controller ?: return
        if (c.isPlaying) c.pause() else c.play()
    }

    fun next() = controller?.seekToNext()
    fun previous() = controller?.seekToPrevious()
    fun seekTo(ms: Long) = controller?.seekTo(ms)

    fun toggleShuffle() {
        controller?.let { it.shuffleModeEnabled = !it.shuffleModeEnabled }
    }

    fun cycleRepeat() {
        val c = controller ?: return
        c.repeatMode = when (c.repeatMode) {
            Player.REPEAT_MODE_OFF -> Player.REPEAT_MODE_ALL
            Player.REPEAT_MODE_ALL -> Player.REPEAT_MODE_ONE
            else -> Player.REPEAT_MODE_OFF
        }
    }

    /** Fila atual (título e artista), para a tela "Tocando agora" */
    fun queue(): List<Pair<Int, MediaItem>> {
        val c = controller ?: return emptyList()
        return (0 until c.mediaItemCount).map { it to c.getMediaItemAt(it) }
    }

    /**
     * Tira da fila músicas do Flow recusadas: a faixa (videoId) e, se `artist` vier, todas do artista.
     * Se a que está tocando sair, o player segue para a próxima que ficou.
     */
    fun removeFlow(videoId: String, artist: String? = null) {
        val c = controller ?: return
        for (i in c.mediaItemCount - 1 downTo 0) {
            val item = c.getMediaItemAt(i)
            if (!item.mediaId.startsWith(PlaybackService.FLOW_PREFIX)) continue
            val sameTrack = item.mediaId == PlaybackService.FLOW_PREFIX + videoId
            val sameArtist = artist != null && item.mediaMetadata.artist?.toString().equals(artist, ignoreCase = true)
            if (sameTrack || sameArtist) c.removeMediaItem(i)
        }
    }

    fun jumpTo(index: Int) {
        controller?.run {
            seekTo(index, 0)
            play()
        }
    }

    fun stop() {
        controller?.run {
            stop()
            clearMediaItems()
        }
    }
}
