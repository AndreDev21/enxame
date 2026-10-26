// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class LEn extends L {
  LEn([String locale = 'en']) : super(locale);

  @override
  String get appName => 'Enxame';

  @override
  String get onboardingTitle => 'Welcome to Enxame';

  @override
  String get onboardingSubtitle =>
      'Chat and share files directly between friends, with no server in the middle. Just pick a name.';

  @override
  String get onboardingNameLabel => 'What should people call you?';

  @override
  String get onboardingStart => 'Get started';

  @override
  String get onboardingStarting => 'Setting up your node…';

  @override
  String startFailed(String error) {
    return 'Could not start: $error';
  }

  @override
  String get navChats => 'Chats';

  @override
  String get navContacts => 'Contacts';

  @override
  String get navContribution => 'Contribution';

  @override
  String get navDiagnostics => 'Diagnostics';

  @override
  String get noChats => 'No chats yet. Add a contact to get started.';

  @override
  String get pickChat => 'Pick a conversation';

  @override
  String get newGroup => 'New group';

  @override
  String get groupNameLabel => 'Group name';

  @override
  String get groupMembersLabel => 'Members';

  @override
  String get create => 'Create';

  @override
  String get cancel => 'Cancel';

  @override
  String get close => 'Close';

  @override
  String get ok => 'Got it';

  @override
  String get groupNeedsContacts =>
      'You need at least one contact to create a group.';

  @override
  String get groupBadge => 'group';

  @override
  String get typeMessage => 'Type a message';

  @override
  String get send => 'Send';

  @override
  String sendFailed(String error) {
    return 'Failed to send: $error';
  }

  @override
  String get deliveredHint => 'delivered';

  @override
  String get pendingHint => 'waiting for delivery';

  @override
  String get noMessages => 'No messages yet. Say hi!';

  @override
  String get you => 'you';

  @override
  String get myInvite => 'My invite';

  @override
  String get myInviteHelp =>
      'Send this invite to whoever you want as a contact. It is valid for a few hours and they only join if you accept.';

  @override
  String get copyInvite => 'Copy invite';

  @override
  String get inviteCopied => 'Invite copied';

  @override
  String get showQr => 'Show QR';

  @override
  String get addContact => 'Add contact';

  @override
  String get addContactHelp => 'Paste the invite the other person sent you.';

  @override
  String get inviteLabel => 'Invite (enxame1…)';

  @override
  String get sendRequest => 'Send request';

  @override
  String get waitingAcceptance =>
      'Request sent. Waiting for the other person to accept…';

  @override
  String contactAdded(String name) {
    return '$name is now your contact.';
  }

  @override
  String get contactRejected => 'The other person declined the request.';

  @override
  String get contactAlready => 'That person is already your contact.';

  @override
  String get requestsTitle => 'Contact requests';

  @override
  String requestFrom(String name) {
    return '$name wants to be your contact';
  }

  @override
  String get requestVerify =>
      'Confirm this fingerprint with the person through another channel before accepting:';

  @override
  String get accept => 'Accept';

  @override
  String get reject => 'Decline';

  @override
  String get contactsTitle => 'Contacts';

  @override
  String get noContacts => 'You have no contacts yet.';

  @override
  String get contactPending => 'waiting';

  @override
  String get fingerprintLabel => 'Fingerprint';

  @override
  String get contributionTitle => 'Contribution to the network';

  @override
  String get contributionHelp =>
      'Your computer helps your contacts by holding messages and files while they are offline. Everything is encrypted: you cannot read what you carry.';

  @override
  String get roleLabel => 'Current role';

  @override
  String get gameMode => 'Game mode';

  @override
  String get gameModeHelp =>
      'Pauses all contribution while you game or use lots of CPU/network.';

  @override
  String get contribute => 'Contribute to the network';

  @override
  String get contributeHelp =>
      'Off means you only use the network and help nobody.';

  @override
  String get servedThisMonth => 'Served this month';

  @override
  String get carrying => 'Holding for contacts';

  @override
  String get itemsDelivered => 'Deliveries made';

  @override
  String get byContact => 'Who you helped';

  @override
  String get nobodyYet => 'Nobody yet.';

  @override
  String messagesUnit(int n) {
    return '$n messages';
  }

  @override
  String get diagnosticsTitle => 'Network diagnostics';

  @override
  String get runDiagnostics => 'Run diagnostics';

  @override
  String get running => 'Measuring…';

  @override
  String get dnsLabel => 'DNS';

  @override
  String get natLabel => 'NAT';

  @override
  String get relayLabel => 'Preferred relay';

  @override
  String get pathDirect => 'direct';

  @override
  String get pathRelay => 'via relay';

  @override
  String get pathMixed => 'mixed';

  @override
  String get pathUnknown => 'unknown';

  @override
  String get unreachable => 'unreachable';

  @override
  String get noContactsToProbe => 'No contacts to probe.';

  @override
  String get groupInfo => 'Group info';

  @override
  String groupCreator(String name) {
    return 'Created by $name';
  }

  @override
  String get seesYourIp => 'sees your IP';

  @override
  String get ipWarningTitle => 'Heads-up: your IP is visible in the group';

  @override
  String get ipWarningBody =>
      'This group has people who are not your contacts. They will see your network address (IP), and you will see theirs:';

  @override
  String joinedGroup(String name) {
    return 'You joined the group $name';
  }

  @override
  String removedFromGroup(String name) {
    return 'You were removed from the group $name';
  }

  @override
  String get notInGroup => 'You are no longer in this group.';

  @override
  String get addMember => 'Add member';

  @override
  String get removeMember => 'Remove';

  @override
  String get renameGroup => 'Rename';

  @override
  String offlineMembers(String names) {
    return 'Offline now: $names';
  }

  @override
  String errorGeneric(String error) {
    return 'Something went wrong: $error';
  }

  @override
  String get navSettings => 'Settings';

  @override
  String get attachFile => 'Send a file';

  @override
  String get dropHere => 'Drop the file here to send it';

  @override
  String get fileDownload => 'Download';

  @override
  String fileSavedAt(String path) {
    return 'Saved to $path';
  }

  @override
  String fileSent(String name) {
    return 'Sending $name…';
  }

  @override
  String fileFailed(String error) {
    return 'File problem: $error';
  }

  @override
  String get groupFilesUnsupported => 'Files are not supported in groups yet.';

  @override
  String get settingsTitle => 'Settings';

  @override
  String get settingsBehavior => 'Behavior';

  @override
  String get closeToTray =>
      'Closing the window keeps Enxame running in the tray';

  @override
  String get closeToTrayHelp =>
      'That way your computer keeps helping your contacts. Off, closing the window quits the app.';

  @override
  String get autostart => 'Open Enxame when the computer starts';

  @override
  String get autostartHelp => 'Off by default.';

  @override
  String get advancedTitle => 'Advanced';

  @override
  String get advancedHelp =>
      'You do not need to touch this. These options are for people with their own server or a troublesome network. They apply the next time the app opens.';

  @override
  String get relayField => 'Relay (default, disabled or your relay URL)';

  @override
  String get relayCaField => 'Relay\'s own CA (PEM file, optional)';

  @override
  String get dnsField =>
      'DNS servers (IPs separated by commas; empty = automatic)';

  @override
  String get downloadDirField => 'Downloads folder (empty = default)';

  @override
  String get mdnsField => 'Discover contacts on the local network (mDNS)';

  @override
  String get saveSettings => 'Save';

  @override
  String get settingsSaved => 'Saved. Changes apply when you reopen the app.';

  @override
  String get trayOpen => 'Open Enxame';

  @override
  String get trayQuit => 'Quit';

  @override
  String trayRole(String role) {
    return 'Role: $role';
  }

  @override
  String get trayUnavailable =>
      'Your desktop may not show the tray icon. If it does not appear, turn off \"Closing the window keeps Enxame running\".';

  @override
  String get callAudio => 'Voice call';

  @override
  String get callVideo => 'Video call';

  @override
  String get callScreen => 'Share my screen';

  @override
  String callCalling(String name) {
    return 'Calling $name…';
  }

  @override
  String callIncomingAudio(String name) {
    return '$name is calling you (voice)';
  }

  @override
  String callIncomingVideo(String name) {
    return '$name is calling you (video)';
  }

  @override
  String callIncomingScreen(String name) {
    return '$name wants to share their screen with you';
  }

  @override
  String get callConnecting => 'Connecting…';

  @override
  String get callRetrying => 'Trying another route…';

  @override
  String get callHangup => 'Hang up';

  @override
  String get callCancel => 'Cancel';

  @override
  String get callMute => 'Mute microphone';

  @override
  String get callUnmute => 'Unmute microphone';

  @override
  String get callCameraOff => 'Turn camera off';

  @override
  String get callCameraOn => 'Turn camera on';

  @override
  String get callShareScreen => 'Share screen';

  @override
  String get callStopShare => 'Stop sharing';

  @override
  String get callSharing => 'You are sharing your screen';

  @override
  String get callWaitingVideo => 'Waiting for video…';

  @override
  String callEnded(String reason) {
    return 'Call ended: $reason';
  }

  @override
  String get pickScreenTitle => 'What do you want to share?';

  @override
  String get pickScreenScreens => 'Screens';

  @override
  String get pickScreenWindows => 'Windows';

  @override
  String get pickScreenNone => 'No screen found.';

  @override
  String callPeerChoosing(String name) {
    return '$name is choosing what to share…';
  }

  @override
  String callYouShareTo(String name) {
    return 'You are sharing your screen with $name';
  }

  @override
  String get callEndedPlain => 'Call ended';

  @override
  String get metricsTitle => 'Call measurements';

  @override
  String metricsInfo(int count) {
    return '$count connections recorded on this computer. Records contain no IPs, names or content; only path type, time to connect and quality. Nothing is sent: exporting writes a file you choose whether to share.';
  }

  @override
  String get metricsExport => 'Export measurements';

  @override
  String metricsExported(String path) {
    return 'File saved to $path';
  }

  @override
  String get metricsNone => 'No connections recorded yet.';
}
