package blinded;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;

/*
 * JSEF-Benchmark L3 — 路径规范化绕过 (Path Canonicalization)
 *
 * 难度：L3（跨方法/路径语义）。Path.resolve / new File(baseDir, userPath)
 * 不会消解 ".." 段，getAbsolutePath() 仅去掉 "." 与尾随分隔符，同样不消解 ".."。
 * 因此写入 baseDir 之外依然成功。
 *
 * CWE-22 (Path Traversal)。安全底线：仅 localhost 演示语义。
 *
 * 修复要点（对照 PathCanonBy.java）：使用 p.toRealPath() / toRealPath(NOFOLLOW_LINKS)
 * 得到规范化路径，并校验其位于 baseDir 规范化前缀之内。
 */
public class PathCanonBx {

    private static final String BASE_DIR = "/var/www/uploads";

    




    public void write(String userPath) throws Exception {
        Path p = Paths.get(BASE_DIR).resolve(userPath);   // 不消解 ".."
        byte[] data = "demo".getBytes();
        /*ANCHOR_1*/
        Files.write(p, data);                              // 可越出 BASE_DIR
    }

    public static void main(String[] args) throws Exception {
        new PathCanonBx().write("../../etc/passwd");
    }
}
