/// The Hershey "Futura Light" single-stroke font (`futural.jhf`, James Hurt's format), as
/// specify-core carries it (from <https://github.com/kamalmostafa/hershey-fonts>). Its licence
/// asks that these acknowledgements go with the font data: the Hershey Fonts were originally
/// created by Dr. A. V. Hershey while working at the U. S. National Bureau of Standards; the
/// format of the font data was originally created by James Hurt, Cognition, Inc., 900
/// Technology Park Drive, Billerica, MA 01821. AP242 presentation carries no characters, only
/// geometry, so a datum letter is drawn as polylines.
const HERSHEY_FUTURAL: &str = r#"12345  1JZ
12345  9MWRFRT RRYQZR[SZRY
12345  6JZNFNM RVFVM
12345 12H]SBLb RYBRb RLOZO RKUYU
12345 27H\PBP_ RTBT_ RYIWGTFPFMGKIKKLMMNOOUQWRXSYUYXWZT[P[MZKX
12345 32F^[FI[ RNFPHPJOLMMKMIKIIJGLFNFPGSHVHYG[F RWTUUTWTYV[X[ZZ[X[VYTWT
12345 35E_\O\N[MZMYNXPVUTXRZP[L[JZIYHWHUISJRQNRMSKSIRGPFNGMIMKNNPQUXWZY[[[\Z\Y
12345  8MWRHQGRFSGSIRKQL
12345 11KYVBTDRGPKOPOTPYR]T`Vb
12345 11KYNBPDRGTKUPUTTYR]P`Nb
12345  9JZRLRX RMOWU RWOMU
12345  6E_RIR[ RIR[R
12345  8NVSWRXQWRVSWSYQ[
12345  3E_IR[R
12345  6NVRVQWRXSWRV
12345  3G][BIb
12345 18H\QFNGLJKOKRLWNZQ[S[VZXWYRYOXJVGSFQF
12345  5H\NJPISFS[
12345 15H\LKLJMHNGPFTFVGWHXJXLWNUQK[Y[
12345 16H\MFXFRNUNWOXPYSYUXXVZS[P[MZLYKW
12345  7H\UFKTZT RUFU[
12345 18H\WFMFLOMNPMSMVNXPYSYUXXVZS[P[MZLYKW
12345 24H\XIWGTFRFOGMJLOLTMXOZR[S[VZXXYUYTXQVOSNRNOOMQLT
12345  6H\YFO[ RKFYF
12345 30H\PFMGLILKMMONSOVPXRYTYWXYWZT[P[MZLYKWKTLRNPQOUNWMXKXIWGTFPF
12345 24H\XMWPURRSQSNRLPKMKLLINGQFRFUGWIXMXRWWUZR[P[MZLX
12345 12NVROQPRQSPRO RRVQWRXSWRV
12345 14NVROQPRQSPRO RSWRXQWRVSWSYQ[
12345  4F^ZIJRZ[
12345  6E_IO[O RIU[U
12345  4F^JIZRJ[
12345 21I[LKLJMHNGPFTFVGWHXJXLWNVORQRT RRYQZR[SZRY
12345 56E`WNVLTKQKOLNMMPMSNUPVSVUUVS RQKOMNPNSOUPV RWKVSVUXVZV\T]Q]O\L[JYHWGTFQFNGLHJJILHOHRIUJWLYNZQ[T[WZYYZX RXKWSWUXV
12345  9I[RFJ[ RRFZ[ RMTWT
12345 24G\KFK[ RKFTFWGXHYJYLXNWOTP RKPTPWQXRYTYWXYWZT[K[
12345 19H]ZKYIWGUFQFOGMILKKNKSLVMXOZQ[U[WZYXZV
12345 16G\KFK[ RKFRFUGWIXKYNYSXVWXUZR[K[
12345 12H[LFL[ RLFYF RLPTP RL[Y[
12345  9HZLFL[ RLFYF RLPTP
12345 23H]ZKYIWGUFQFOGMILKKNKSLVMXOZQ[U[WZYXZVZS RUSZS
12345  9G]KFK[ RYFY[ RKPYP
12345  3NVRFR[
12345 11JZVFVVUYTZR[P[NZMYLVLT
12345  9G\KFK[ RYFKT RPOY[
12345  6HYLFL[ RL[X[
12345 12F^JFJ[ RJFR[ RZFR[ RZFZ[
12345  9G]KFK[ RKFY[ RYFY[
12345 22G]PFNGLIKKJNJSKVLXNZP[T[VZXXYVZSZNYKXIVGTFPF
12345 14G\KFK[ RKFTFWGXHYJYMXOWPTQKQ
12345 25G]PFNGLIKKJNJSKVLXNZP[T[VZXXYVZSZNYKXIVGTFPF RSWY]
12345 17G\KFK[ RKFTFWGXHYJYLXNWOTPKP RRPY[
12345 21H\YIWGTFPFMGKIKKLMMNOOUQWRXSYUYXWZT[P[MZKX
12345  6JZRFR[ RKFYF
12345 11G]KFKULXNZQ[S[VZXXYUYF
12345  6I[JFR[ RZFR[
12345 12F^HFM[ RRFM[ RRFW[ R\FW[
12345  6H\KFY[ RYFK[
12345  7I[JFRPR[ RZFRP
12345  9H\YFK[ RKFYF RK[Y[
12345 12KYOBOb RPBPb ROBVB RObVb
12345  3KYKFY^
12345 12KYTBTb RUBUb RNBUB RNbUb
12345  6JZRDJR RRDZR
12345  3I[Ib[b
12345  8NVSKQMQORPSORNQO
12345 18I\XMX[ RXPVNTMQMONMPLSLUMXOZQ[T[VZXX
12345 18H[LFL[ RLPNNPMSMUNWPXSXUWXUZS[P[NZLX
12345 15I[XPVNTMQMONMPLSLUMXOZQ[T[VZXX
12345 18I\XFX[ RXPVNTMQMONMPLSLUMXOZQ[T[VZXX
12345 18I[LSXSXQWOVNTMQMONMPLSLUMXOZQ[T[VZXX
12345  9MYWFUFSGRJR[ ROMVM
12345 23I\XMX]W`VaTbQbOa RXPVNTMQMONMPLSLUMXOZQ[T[VZXX
12345 11I\MFM[ RMQPNRMUMWNXQX[
12345  9NVQFRGSFREQF RRMR[
12345 12MWRFSGTFSERF RSMS^RaPbNb
12345  9IZMFM[ RWMMW RQSX[
12345  3NVRFR[
12345 19CaGMG[ RGQJNLMOMQNRQR[ RRQUNWMZM\N]Q][
12345 11I\MMM[ RMQPNRMUMWNXQX[
12345 18I\QMONMPLSLUMXOZQ[T[VZXXYUYSXPVNTMQM
12345 18H[LMLb RLPNNPMSMUNWPXSXUWXUZS[P[NZLX
12345 18I\XMXb RXPVNTMQMONMPLSLUMXOZQ[T[VZXX
12345  9KXOMO[ ROSPPRNTMWM
12345 18J[XPWNTMQMNNMPNRPSUTWUXWXXWZT[Q[NZMX
12345  9MYRFRWSZU[W[ ROMVM
12345 11I\MMMWNZP[S[UZXW RXMX[
12345  6JZLMR[ RXMR[
12345 12G]JMN[ RRMN[ RRMV[ RZMV[
12345  6J[MMX[ RXMM[
12345 10JZLMR[ RXMR[P_NaLbKb
12345  9J[XMM[ RMMXM RM[X[
12345 40KYTBRCQDPFPHQJRKSMSOQQ RRCQEQGRISJTLTNSPORSTTVTXSZR[Q]Q_Ra RQSSUSWRYQZP\P^Q`RaTb
12345  3NVRBRb
12345 40KYPBRCSDTFTHSJRKQMQOSQ RRCSESGRIQJPLPNQPURQTPVPXQZR[S]S_Ra RSSQUQWRYSZT\T^S`RaPb
12345 24F^IUISJPLONOPPTSVTXTZS[Q RISJQLPNPPQTTVUXUZT[Q[O
12345 35JZJFJ[K[KFLFL[M[MFNFN[O[OFPFP[Q[QFRFR[S[SFTFT[U[UFVFV[W[WFXFX[Y[YFZFZ[
"#;

/// One glyph: its advance and its strokes, in font units (capital height 21, baseline 0).
pub(crate) type Glyph = (f64, Vec<Vec<(f64, f64)>>);

/// Character → glyph for ASCII 32 to 126: a 5-character number, a 3-character vertex count,
/// then pairs of characters each offset from 'R' (the left and right bearings, then x, y, with
/// " R" lifting the pen).
fn hershey_glyphs() -> Vec<Glyph> {
    let raw: Vec<u8> = HERSHEY_FUTURAL.bytes().filter(|&b| b != b'\n').collect();
    let mut glyphs = Vec::new();
    let mut i = 0;
    let at = |b: u8| f64::from(b) - 82.0;
    while i + 8 <= raw.len() {
        let count: usize = std::str::from_utf8(&raw[i + 5..i + 8])
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(0);
        let body = &raw[i + 8..(i + 8 + 2 * count).min(raw.len())];
        i += 8 + 2 * count;
        if body.len() < 2 {
            continue;
        }
        let (left, right) = (at(body[0]), at(body[1]));
        let mut strokes = Vec::new();
        let mut current = Vec::new();
        for pair in body[2..].chunks(2) {
            if pair == b" R" {
                strokes.push(std::mem::take(&mut current));
            } else if let [x, y] = pair {
                current.push((at(*x) - left, 9.0 - at(*y)));
            }
        }
        strokes.push(current);
        strokes.retain(|s: &Vec<(f64, f64)>| s.len() > 1);
        glyphs.push((right - left, strokes));
    }
    glyphs
}

/// The strokes of `text` at capital `height`, from x = 0 on the baseline, and its width; a
/// character the font lacks is drawn as '?'.
#[cfg(not(feature = "presentation"))]
pub(crate) fn lettering(text: &str, height: f64) -> (Vec<Vec<(f64, f64)>>, f64) {
    let glyphs = hershey_glyphs();
    let scale = height / 21.0;
    let mut out = Vec::new();
    let mut x = 0.0;
    for c in text.chars() {
        let k = (c as usize)
            .checked_sub(32)
            .filter(|&k| k < 95 && k < glyphs.len())
            .unwrap_or(31);
        let (advance, strokes) = &glyphs[k];
        for s in strokes {
            out.push(
                s.iter()
                    .map(|(px, py)| ((x + px) * scale, py * scale))
                    .collect(),
            );
        }
        x += advance;
    }
    (out, x * scale)
}

/// The public-domain ASCII glyph, without substitution for an unsupported character.
#[cfg(feature = "presentation")]
pub(crate) fn glyph(c: char) -> Option<Glyph> {
    static GLYPHS: std::sync::OnceLock<Vec<Glyph>> = std::sync::OnceLock::new();
    let index = (c as usize).checked_sub(32).filter(|&i| i < 95)?;
    GLYPHS.get_or_init(hershey_glyphs).get(index).cloned()
}
