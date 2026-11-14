package com.rymplayer.ui

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Shader
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import androidx.core.content.FileProvider
import androidx.core.graphics.drawable.toBitmap
import com.rymplayer.data.Track
import com.rymplayer.data.thumbSized
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/**
 * Lyric cards: the picked lines (1 to 3) on a picture made for sharing: the song's cover, the
 * lines large, the song and artist, on the theme's colour fading to black, and RyM's name.
 */
object LyricCard {
    private const val W = 1080
    private const val H = 1350

    /** Makes the card and opens Android's share sheet with it. */
    suspend fun share(ctx: Context, t: Track, lines: List<String>, accent: Int) {
        val file = withContext(Dispatchers.IO) {
            val card = draw(ctx, t, lines, accent)
            val dir = File(ctx.cacheDir, "cards").apply { mkdirs() }
            // One card at a time is kept.
            dir.listFiles()?.forEach { it.delete() }
            File(dir, "lyrics-${t.videoId.filter { it.isLetterOrDigit() }.take(20)}.png").also { f ->
                f.outputStream().use { card.compress(Bitmap.CompressFormat.PNG, 100, it) }
                card.recycle()
            }
        }
        val uri = FileProvider.getUriForFile(ctx, "${ctx.packageName}.files", file)
        val send = Intent(Intent.ACTION_SEND).setType("image/png").putExtra(Intent.EXTRA_STREAM, uri)
            .putExtra(Intent.EXTRA_TEXT, "${t.title} · ${t.artist}")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        ctx.startActivity(Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    private suspend fun cover(ctx: Context, t: Track): Bitmap? {
        val req = coil.request.ImageRequest.Builder(ctx).data(thumbSized(t.thumb, 1080)).allowHardware(false).build()
        val result = coil.ImageLoader(ctx).execute(req) as? coil.request.SuccessResult ?: return null
        return result.drawable.toBitmap()
    }

    private suspend fun draw(ctx: Context, t: Track, lines: List<String>, accent: Int): Bitmap {
        val bmp = Bitmap.createBitmap(W, H, Bitmap.Config.ARGB_8888)
        val c = Canvas(bmp)
        // The theme's colour, darkened, fading to black.
        val top = android.graphics.Color.argb(255, android.graphics.Color.red(accent) * 6 / 10, android.graphics.Color.green(accent) * 6 / 10, android.graphics.Color.blue(accent) * 6 / 10)
        c.drawRect(0f, 0f, W.toFloat(), H.toFloat(), Paint().apply { shader = LinearGradient(0f, 0f, 0f, H.toFloat(), top, android.graphics.Color.rgb(10, 10, 12), Shader.TileMode.CLAMP) })
        // The cover, rounded.
        val side = 560f
        val left = (W - side) / 2
        val coverTop = 90f
        cover(ctx, t)?.let { art ->
            val box = RectF(left, coverTop, left + side, coverTop + side)
            c.save()
            c.clipPath(Path().apply { addRoundRect(box, 36f, 36f, Path.Direction.CW) })
            c.drawBitmap(art, null, box, Paint(Paint.FILTER_BITMAP_FLAG))
            c.restore()
            art.recycle()
        }
        // The lines, large (Arabic reads right to left on its own).
        val text = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = android.graphics.Color.WHITE
            textSize = if (lines.joinToString(" ").length > 90) 50f else 60f
            isFakeBoldText = true
        }
        val body = StaticLayout.Builder.obtain(lines.joinToString("\n"), 0, lines.joinToString("\n").length, text, W - 160)
            .setAlignment(Layout.Alignment.ALIGN_CENTER).setLineSpacing(10f, 1f).setMaxLines(8).build()
        c.save()
        c.translate(80f, coverTop + side + 70f)
        body.draw(c)
        c.restore()
        // The song and artist, then RyM's name.
        val small = TextPaint(Paint.ANTI_ALIAS_FLAG).apply { color = android.graphics.Color.argb(200, 255, 255, 255); textSize = 36f; textAlign = Paint.Align.CENTER }
        val name = "${t.title} · ${t.artist}".let { if (it.length > 60) it.take(58) + "…" else it }
        c.drawText(name, W / 2f, H - 130f, small)
        small.color = android.graphics.Color.argb(140, 255, 255, 255)
        small.textSize = 30f
        small.isFakeBoldText = true
        c.drawText("RyM", W / 2f, H - 70f, small)
        return bmp
    }
}
