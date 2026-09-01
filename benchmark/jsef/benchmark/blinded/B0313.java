package blinded;

import org.apache.commons.compress.archivers.tar.TarArchiveEntry;
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;







public class TarSlipBy {

    


    static void untar(InputStream in, String dest) throws IOException {
        TarArchiveInputStream tar = new TarArchiveInputStream(in);
        Path target = Paths.get(dest).toAbsolutePath().normalize();
        TarArchiveEntry e;
        while ((e = tar.getNextTarEntry()) != null) {
            Path out = target.resolve(e.getName()).normalize(); // 规范化消除 ..
            if (e.getName().startsWith("/") || !out.startsWith(target)) {
                // 绝对路径或路径穿越到目标目录之外，直接拒绝
                throw new IOException("tar entry escapes destination: " + e.getName());
            }
            /*ANCHOR_1*/
            Files.copy(tar, out); // 已校验 out 在目标目录内，写入安全
        }
    }
}
