package br.com.ilmar.ilmarfy.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import br.com.ilmar.ilmarfy.core.Core
import kotlinx.coroutines.delay
import uniffi.ilmarfy_ffi.Track
import uniffi.ilmarfy_ffi.TrackOrder

/** Busca na biblioteca local (ignora acentos; funciona offline). Sem texto, lista tudo de A a Z. */
@Composable
fun SearchScreen(onMessage: (String) -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    var results by remember { mutableStateOf<List<Track>>(emptyList()) }
    val version by Core.version.collectAsState()
    LaunchedEffect(query, version) {
        delay(150) // espera parar de digitar
        results = runCatching {
            Core.io { if (query.isBlank()) it.tracks(TrackOrder.TITLE) else it.search(query, 200u) }
        }.getOrDefault(emptyList())
    }
    Column(Modifier.fillMaxSize()) {
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            placeholder = { Text("Música, artista ou álbum") },
            leadingIcon = { Icon(Icons.Rounded.Search, null) },
            trailingIcon = {
                if (query.isNotEmpty()) IconButton(onClick = { query = "" }) { Icon(Icons.Rounded.Close, "Limpar") }
            },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(16.dp),
        )
        if (query.isNotBlank() && results.isEmpty()) {
            Text("Nada encontrado para \"$query\".", color = Subtext, modifier = Modifier.padding(16.dp))
        }
        TrackList(results, onMessage)
    }
}
