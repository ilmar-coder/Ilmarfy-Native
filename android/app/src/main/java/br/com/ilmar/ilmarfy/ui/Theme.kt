package br.com.ilmar.ilmarfy.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

val IlmarfyGreen = Color(0xFF1DB954)
val Subtext = Color(0xFFB3B3B3)

private val colors = darkColorScheme(
    primary = IlmarfyGreen,
    onPrimary = Color.Black,
    secondary = IlmarfyGreen,
    background = Color(0xFF121212),
    onBackground = Color.White,
    surface = Color(0xFF181818),
    onSurface = Color.White,
    surfaceVariant = Color(0xFF282828),
    onSurfaceVariant = Subtext,
    surfaceContainer = Color(0xFF1E1E1E),
    surfaceContainerHigh = Color(0xFF282828),
    secondaryContainer = Color(0xFF2A2A2A),
    onSecondaryContainer = Color.White,
    error = Color(0xFFFF6B6B),
)

@Composable
fun IlmarfyTheme(content: @Composable () -> Unit) = MaterialTheme(colorScheme = colors, content = content)
