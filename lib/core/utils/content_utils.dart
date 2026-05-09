import 'dart:convert';
import 'package:crypto/crypto.dart';

class ContentUtils {
  static String calculateHash(String content) {
    var bytes = utf8.encode(content);
    var digest = sha256.convert(bytes);
    return digest.toString();
  }

  static int calculateWordCount(String content) {
    // For MVP, just split by whitespace.
    // A robust version would need CJK character counting.
    if (content.trim().isEmpty) return 0;

    // Simple CJK + Word split heuristic for MVP
    int count = 0;
    final words = content.split(RegExp(r'\s+'));
    for (var word in words) {
      if (word.isNotEmpty) {
         // rough approximation: if it has CJK characters, count them individually
         if (RegExp(r'[\u4e00-\u9fa5]').hasMatch(word)) {
           count += word.runes.length;
         } else {
           count += 1;
         }
      }
    }
    return count;
  }
}
