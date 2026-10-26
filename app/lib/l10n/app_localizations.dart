import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_pt.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of L
/// returned by `L.of(context)`.
///
/// Applications need to include `L.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: L.localizationsDelegates,
///   supportedLocales: L.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the L.supportedLocales
/// property.
abstract class L {
  L(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static L of(BuildContext context) {
    return Localizations.of<L>(context, L)!;
  }

  static const LocalizationsDelegate<L> delegate = _LDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('pt'),
  ];

  /// No description provided for @appName.
  ///
  /// In pt, this message translates to:
  /// **'Enxame'**
  String get appName;

  /// No description provided for @onboardingTitle.
  ///
  /// In pt, this message translates to:
  /// **'Bem-vindo ao Enxame'**
  String get onboardingTitle;

  /// No description provided for @onboardingSubtitle.
  ///
  /// In pt, this message translates to:
  /// **'Conversas e arquivos direto entre amigos, sem servidor no meio. Basta escolher um nome.'**
  String get onboardingSubtitle;

  /// No description provided for @onboardingNameLabel.
  ///
  /// In pt, this message translates to:
  /// **'Como você quer ser chamado?'**
  String get onboardingNameLabel;

  /// No description provided for @onboardingStart.
  ///
  /// In pt, this message translates to:
  /// **'Começar'**
  String get onboardingStart;

  /// No description provided for @onboardingStarting.
  ///
  /// In pt, this message translates to:
  /// **'Preparando seu nó…'**
  String get onboardingStarting;

  /// No description provided for @startFailed.
  ///
  /// In pt, this message translates to:
  /// **'Não foi possível iniciar: {error}'**
  String startFailed(String error);

  /// No description provided for @navChats.
  ///
  /// In pt, this message translates to:
  /// **'Conversas'**
  String get navChats;

  /// No description provided for @navContacts.
  ///
  /// In pt, this message translates to:
  /// **'Contatos'**
  String get navContacts;

  /// No description provided for @navContribution.
  ///
  /// In pt, this message translates to:
  /// **'Contribuição'**
  String get navContribution;

  /// No description provided for @navDiagnostics.
  ///
  /// In pt, this message translates to:
  /// **'Diagnóstico'**
  String get navDiagnostics;

  /// No description provided for @noChats.
  ///
  /// In pt, this message translates to:
  /// **'Nenhuma conversa ainda. Adicione um contato para começar.'**
  String get noChats;

  /// No description provided for @pickChat.
  ///
  /// In pt, this message translates to:
  /// **'Escolha uma conversa'**
  String get pickChat;

  /// No description provided for @newGroup.
  ///
  /// In pt, this message translates to:
  /// **'Novo grupo'**
  String get newGroup;

  /// No description provided for @groupNameLabel.
  ///
  /// In pt, this message translates to:
  /// **'Nome do grupo'**
  String get groupNameLabel;

  /// No description provided for @groupMembersLabel.
  ///
  /// In pt, this message translates to:
  /// **'Membros'**
  String get groupMembersLabel;

  /// No description provided for @create.
  ///
  /// In pt, this message translates to:
  /// **'Criar'**
  String get create;

  /// No description provided for @cancel.
  ///
  /// In pt, this message translates to:
  /// **'Cancelar'**
  String get cancel;

  /// No description provided for @close.
  ///
  /// In pt, this message translates to:
  /// **'Fechar'**
  String get close;

  /// No description provided for @ok.
  ///
  /// In pt, this message translates to:
  /// **'Entendi'**
  String get ok;

  /// No description provided for @groupNeedsContacts.
  ///
  /// In pt, this message translates to:
  /// **'Você precisa de pelo menos um contato para criar um grupo.'**
  String get groupNeedsContacts;

  /// No description provided for @groupBadge.
  ///
  /// In pt, this message translates to:
  /// **'grupo'**
  String get groupBadge;

  /// No description provided for @typeMessage.
  ///
  /// In pt, this message translates to:
  /// **'Escreva uma mensagem'**
  String get typeMessage;

  /// No description provided for @send.
  ///
  /// In pt, this message translates to:
  /// **'Enviar'**
  String get send;

  /// No description provided for @sendFailed.
  ///
  /// In pt, this message translates to:
  /// **'Falha ao enviar: {error}'**
  String sendFailed(String error);

  /// No description provided for @deliveredHint.
  ///
  /// In pt, this message translates to:
  /// **'entregue'**
  String get deliveredHint;

  /// No description provided for @pendingHint.
  ///
  /// In pt, this message translates to:
  /// **'aguardando entrega'**
  String get pendingHint;

  /// No description provided for @noMessages.
  ///
  /// In pt, this message translates to:
  /// **'Nenhuma mensagem ainda. Diga oi!'**
  String get noMessages;

  /// No description provided for @you.
  ///
  /// In pt, this message translates to:
  /// **'você'**
  String get you;

  /// No description provided for @myInvite.
  ///
  /// In pt, this message translates to:
  /// **'Meu convite'**
  String get myInvite;

  /// No description provided for @myInviteHelp.
  ///
  /// In pt, this message translates to:
  /// **'Mande este convite para quem você quer como contato. Ele vale por algumas horas e a pessoa só entra se você aceitar.'**
  String get myInviteHelp;

  /// No description provided for @copyInvite.
  ///
  /// In pt, this message translates to:
  /// **'Copiar convite'**
  String get copyInvite;

  /// No description provided for @inviteCopied.
  ///
  /// In pt, this message translates to:
  /// **'Convite copiado'**
  String get inviteCopied;

  /// No description provided for @showQr.
  ///
  /// In pt, this message translates to:
  /// **'Mostrar QR'**
  String get showQr;

  /// No description provided for @addContact.
  ///
  /// In pt, this message translates to:
  /// **'Adicionar contato'**
  String get addContact;

  /// No description provided for @addContactHelp.
  ///
  /// In pt, this message translates to:
  /// **'Cole o convite que a outra pessoa mandou.'**
  String get addContactHelp;

  /// No description provided for @inviteLabel.
  ///
  /// In pt, this message translates to:
  /// **'Convite (enxame1…)'**
  String get inviteLabel;

  /// No description provided for @sendRequest.
  ///
  /// In pt, this message translates to:
  /// **'Enviar pedido'**
  String get sendRequest;

  /// No description provided for @waitingAcceptance.
  ///
  /// In pt, this message translates to:
  /// **'Pedido enviado. Aguardando a outra pessoa aceitar…'**
  String get waitingAcceptance;

  /// No description provided for @contactAdded.
  ///
  /// In pt, this message translates to:
  /// **'{name} agora é seu contato.'**
  String contactAdded(String name);

  /// No description provided for @contactRejected.
  ///
  /// In pt, this message translates to:
  /// **'A outra pessoa recusou o pedido.'**
  String get contactRejected;

  /// No description provided for @contactAlready.
  ///
  /// In pt, this message translates to:
  /// **'Essa pessoa já é seu contato.'**
  String get contactAlready;

  /// No description provided for @requestsTitle.
  ///
  /// In pt, this message translates to:
  /// **'Pedidos de contato'**
  String get requestsTitle;

  /// No description provided for @requestFrom.
  ///
  /// In pt, this message translates to:
  /// **'{name} quer ser seu contato'**
  String requestFrom(String name);

  /// No description provided for @requestVerify.
  ///
  /// In pt, this message translates to:
  /// **'Confirme esta impressão digital com a pessoa por outro canal antes de aceitar:'**
  String get requestVerify;

  /// No description provided for @accept.
  ///
  /// In pt, this message translates to:
  /// **'Aceitar'**
  String get accept;

  /// No description provided for @reject.
  ///
  /// In pt, this message translates to:
  /// **'Recusar'**
  String get reject;

  /// No description provided for @contactsTitle.
  ///
  /// In pt, this message translates to:
  /// **'Contatos'**
  String get contactsTitle;

  /// No description provided for @noContacts.
  ///
  /// In pt, this message translates to:
  /// **'Você ainda não tem contatos.'**
  String get noContacts;

  /// No description provided for @contactPending.
  ///
  /// In pt, this message translates to:
  /// **'aguardando'**
  String get contactPending;

  /// No description provided for @fingerprintLabel.
  ///
  /// In pt, this message translates to:
  /// **'Impressão digital'**
  String get fingerprintLabel;

  /// No description provided for @contributionTitle.
  ///
  /// In pt, this message translates to:
  /// **'Contribuição para a rede'**
  String get contributionTitle;

  /// No description provided for @contributionHelp.
  ///
  /// In pt, this message translates to:
  /// **'Seu computador ajuda seus contatos guardando mensagens e arquivos enquanto eles estão offline. Tudo cifrado: você não lê nada do que carrega.'**
  String get contributionHelp;

  /// No description provided for @roleLabel.
  ///
  /// In pt, this message translates to:
  /// **'Papel atual'**
  String get roleLabel;

  /// No description provided for @gameMode.
  ///
  /// In pt, this message translates to:
  /// **'Modo jogo'**
  String get gameMode;

  /// No description provided for @gameModeHelp.
  ///
  /// In pt, this message translates to:
  /// **'Pausa toda a contribuição enquanto você joga ou usa muita CPU/rede.'**
  String get gameModeHelp;

  /// No description provided for @contribute.
  ///
  /// In pt, this message translates to:
  /// **'Contribuir com a rede'**
  String get contribute;

  /// No description provided for @contributeHelp.
  ///
  /// In pt, this message translates to:
  /// **'Desligado, você só usa a rede e não ajuda ninguém.'**
  String get contributeHelp;

  /// No description provided for @servedThisMonth.
  ///
  /// In pt, this message translates to:
  /// **'Servido neste mês'**
  String get servedThisMonth;

  /// No description provided for @carrying.
  ///
  /// In pt, this message translates to:
  /// **'Guardando para contatos'**
  String get carrying;

  /// No description provided for @itemsDelivered.
  ///
  /// In pt, this message translates to:
  /// **'Entregas feitas'**
  String get itemsDelivered;

  /// No description provided for @byContact.
  ///
  /// In pt, this message translates to:
  /// **'Quem você ajudou'**
  String get byContact;

  /// No description provided for @nobodyYet.
  ///
  /// In pt, this message translates to:
  /// **'Ninguém ainda.'**
  String get nobodyYet;

  /// No description provided for @messagesUnit.
  ///
  /// In pt, this message translates to:
  /// **'{n} mensagens'**
  String messagesUnit(int n);

  /// No description provided for @diagnosticsTitle.
  ///
  /// In pt, this message translates to:
  /// **'Diagnóstico de rede'**
  String get diagnosticsTitle;

  /// No description provided for @runDiagnostics.
  ///
  /// In pt, this message translates to:
  /// **'Executar diagnóstico'**
  String get runDiagnostics;

  /// No description provided for @running.
  ///
  /// In pt, this message translates to:
  /// **'Medindo…'**
  String get running;

  /// No description provided for @dnsLabel.
  ///
  /// In pt, this message translates to:
  /// **'DNS'**
  String get dnsLabel;

  /// No description provided for @natLabel.
  ///
  /// In pt, this message translates to:
  /// **'NAT'**
  String get natLabel;

  /// No description provided for @relayLabel.
  ///
  /// In pt, this message translates to:
  /// **'Relay preferido'**
  String get relayLabel;

  /// No description provided for @pathDirect.
  ///
  /// In pt, this message translates to:
  /// **'direto'**
  String get pathDirect;

  /// No description provided for @pathRelay.
  ///
  /// In pt, this message translates to:
  /// **'via relay'**
  String get pathRelay;

  /// No description provided for @pathMixed.
  ///
  /// In pt, this message translates to:
  /// **'misto'**
  String get pathMixed;

  /// No description provided for @pathUnknown.
  ///
  /// In pt, this message translates to:
  /// **'desconhecido'**
  String get pathUnknown;

  /// No description provided for @unreachable.
  ///
  /// In pt, this message translates to:
  /// **'inacessível'**
  String get unreachable;

  /// No description provided for @noContactsToProbe.
  ///
  /// In pt, this message translates to:
  /// **'Sem contatos para sondar.'**
  String get noContactsToProbe;

  /// No description provided for @groupInfo.
  ///
  /// In pt, this message translates to:
  /// **'Informações do grupo'**
  String get groupInfo;

  /// No description provided for @groupCreator.
  ///
  /// In pt, this message translates to:
  /// **'Criado por {name}'**
  String groupCreator(String name);

  /// No description provided for @seesYourIp.
  ///
  /// In pt, this message translates to:
  /// **'vê seu IP'**
  String get seesYourIp;

  /// No description provided for @ipWarningTitle.
  ///
  /// In pt, this message translates to:
  /// **'Atenção: IP visível no grupo'**
  String get ipWarningTitle;

  /// No description provided for @ipWarningBody.
  ///
  /// In pt, this message translates to:
  /// **'Neste grupo há pessoas que não são seus contatos. Elas passam a ver o seu endereço de rede (IP), e você o delas:'**
  String get ipWarningBody;

  /// No description provided for @joinedGroup.
  ///
  /// In pt, this message translates to:
  /// **'Você entrou no grupo {name}'**
  String joinedGroup(String name);

  /// No description provided for @removedFromGroup.
  ///
  /// In pt, this message translates to:
  /// **'Você foi removido do grupo {name}'**
  String removedFromGroup(String name);

  /// No description provided for @notInGroup.
  ///
  /// In pt, this message translates to:
  /// **'Você não está mais neste grupo.'**
  String get notInGroup;

  /// No description provided for @addMember.
  ///
  /// In pt, this message translates to:
  /// **'Adicionar membro'**
  String get addMember;

  /// No description provided for @removeMember.
  ///
  /// In pt, this message translates to:
  /// **'Remover'**
  String get removeMember;

  /// No description provided for @renameGroup.
  ///
  /// In pt, this message translates to:
  /// **'Renomear'**
  String get renameGroup;

  /// No description provided for @offlineMembers.
  ///
  /// In pt, this message translates to:
  /// **'Offline agora: {names}'**
  String offlineMembers(String names);

  /// No description provided for @errorGeneric.
  ///
  /// In pt, this message translates to:
  /// **'Algo deu errado: {error}'**
  String errorGeneric(String error);

  /// No description provided for @navSettings.
  ///
  /// In pt, this message translates to:
  /// **'Configurações'**
  String get navSettings;

  /// No description provided for @attachFile.
  ///
  /// In pt, this message translates to:
  /// **'Enviar arquivo'**
  String get attachFile;

  /// No description provided for @dropHere.
  ///
  /// In pt, this message translates to:
  /// **'Solte o arquivo aqui para enviar'**
  String get dropHere;

  /// No description provided for @fileDownload.
  ///
  /// In pt, this message translates to:
  /// **'Baixar'**
  String get fileDownload;

  /// No description provided for @fileSavedAt.
  ///
  /// In pt, this message translates to:
  /// **'Salvo em {path}'**
  String fileSavedAt(String path);

  /// No description provided for @fileSent.
  ///
  /// In pt, this message translates to:
  /// **'Enviando {name}…'**
  String fileSent(String name);

  /// No description provided for @fileFailed.
  ///
  /// In pt, this message translates to:
  /// **'Falha com o arquivo: {error}'**
  String fileFailed(String error);

  /// No description provided for @groupFilesUnsupported.
  ///
  /// In pt, this message translates to:
  /// **'Arquivos ainda não são suportados em grupos.'**
  String get groupFilesUnsupported;

  /// No description provided for @settingsTitle.
  ///
  /// In pt, this message translates to:
  /// **'Configurações'**
  String get settingsTitle;

  /// No description provided for @settingsBehavior.
  ///
  /// In pt, this message translates to:
  /// **'Comportamento'**
  String get settingsBehavior;

  /// No description provided for @closeToTray.
  ///
  /// In pt, this message translates to:
  /// **'Fechar a janela mantém o Enxame ativo na bandeja'**
  String get closeToTray;

  /// No description provided for @closeToTrayHelp.
  ///
  /// In pt, this message translates to:
  /// **'Assim seu computador continua ajudando seus contatos. Desligado, fechar a janela encerra o app.'**
  String get closeToTrayHelp;

  /// No description provided for @autostart.
  ///
  /// In pt, this message translates to:
  /// **'Abrir o Enxame ao ligar o computador'**
  String get autostart;

  /// No description provided for @autostartHelp.
  ///
  /// In pt, this message translates to:
  /// **'Desligado por padrão.'**
  String get autostartHelp;

  /// No description provided for @advancedTitle.
  ///
  /// In pt, this message translates to:
  /// **'Avançado'**
  String get advancedTitle;

  /// No description provided for @advancedHelp.
  ///
  /// In pt, this message translates to:
  /// **'Você não precisa mexer aqui. Estas opções são para quem usa um servidor próprio ou tem uma rede com problemas. Valem na próxima vez que o app abrir.'**
  String get advancedHelp;

  /// No description provided for @relayField.
  ///
  /// In pt, this message translates to:
  /// **'Relay (default, disabled ou URL do seu relay)'**
  String get relayField;

  /// No description provided for @relayCaField.
  ///
  /// In pt, this message translates to:
  /// **'CA própria do relay (arquivo PEM, opcional)'**
  String get relayCaField;

  /// No description provided for @dnsField.
  ///
  /// In pt, this message translates to:
  /// **'Servidores DNS (IPs separados por vírgula; vazio = automático)'**
  String get dnsField;

  /// No description provided for @downloadDirField.
  ///
  /// In pt, this message translates to:
  /// **'Pasta de downloads (vazio = padrão)'**
  String get downloadDirField;

  /// No description provided for @mdnsField.
  ///
  /// In pt, this message translates to:
  /// **'Descobrir contatos na rede local (mDNS)'**
  String get mdnsField;

  /// No description provided for @saveSettings.
  ///
  /// In pt, this message translates to:
  /// **'Salvar'**
  String get saveSettings;

  /// No description provided for @settingsSaved.
  ///
  /// In pt, this message translates to:
  /// **'Salvo. As mudanças valem quando você reabrir o app.'**
  String get settingsSaved;

  /// No description provided for @trayOpen.
  ///
  /// In pt, this message translates to:
  /// **'Abrir o Enxame'**
  String get trayOpen;

  /// No description provided for @trayQuit.
  ///
  /// In pt, this message translates to:
  /// **'Sair'**
  String get trayQuit;

  /// No description provided for @trayRole.
  ///
  /// In pt, this message translates to:
  /// **'Papel: {role}'**
  String trayRole(String role);

  /// No description provided for @trayUnavailable.
  ///
  /// In pt, this message translates to:
  /// **'Sua área de trabalho pode não mostrar o ícone da bandeja. Se não aparecer, desligue \"Fechar a janela mantém o Enxame ativo\".'**
  String get trayUnavailable;

  /// No description provided for @callAudio.
  ///
  /// In pt, this message translates to:
  /// **'Chamada de voz'**
  String get callAudio;

  /// No description provided for @callVideo.
  ///
  /// In pt, this message translates to:
  /// **'Chamada de vídeo'**
  String get callVideo;

  /// No description provided for @callScreen.
  ///
  /// In pt, this message translates to:
  /// **'Transmitir minha tela'**
  String get callScreen;

  /// No description provided for @callCalling.
  ///
  /// In pt, this message translates to:
  /// **'Chamando {name}…'**
  String callCalling(String name);

  /// No description provided for @callIncomingAudio.
  ///
  /// In pt, this message translates to:
  /// **'{name} está te chamando (voz)'**
  String callIncomingAudio(String name);

  /// No description provided for @callIncomingVideo.
  ///
  /// In pt, this message translates to:
  /// **'{name} está te chamando (vídeo)'**
  String callIncomingVideo(String name);

  /// No description provided for @callIncomingScreen.
  ///
  /// In pt, this message translates to:
  /// **'{name} quer transmitir a tela para você'**
  String callIncomingScreen(String name);

  /// No description provided for @callConnecting.
  ///
  /// In pt, this message translates to:
  /// **'Conectando…'**
  String get callConnecting;

  /// No description provided for @callRetrying.
  ///
  /// In pt, this message translates to:
  /// **'Tentando outro caminho…'**
  String get callRetrying;

  /// No description provided for @callHangup.
  ///
  /// In pt, this message translates to:
  /// **'Desligar'**
  String get callHangup;

  /// No description provided for @callCancel.
  ///
  /// In pt, this message translates to:
  /// **'Cancelar'**
  String get callCancel;

  /// No description provided for @callMute.
  ///
  /// In pt, this message translates to:
  /// **'Silenciar microfone'**
  String get callMute;

  /// No description provided for @callUnmute.
  ///
  /// In pt, this message translates to:
  /// **'Ativar microfone'**
  String get callUnmute;

  /// No description provided for @callCameraOff.
  ///
  /// In pt, this message translates to:
  /// **'Desligar câmera'**
  String get callCameraOff;

  /// No description provided for @callCameraOn.
  ///
  /// In pt, this message translates to:
  /// **'Ligar câmera'**
  String get callCameraOn;

  /// No description provided for @callShareScreen.
  ///
  /// In pt, this message translates to:
  /// **'Compartilhar tela'**
  String get callShareScreen;

  /// No description provided for @callStopShare.
  ///
  /// In pt, this message translates to:
  /// **'Parar de compartilhar'**
  String get callStopShare;

  /// No description provided for @callSharing.
  ///
  /// In pt, this message translates to:
  /// **'Você está transmitindo sua tela'**
  String get callSharing;

  /// No description provided for @callWaitingVideo.
  ///
  /// In pt, this message translates to:
  /// **'Aguardando o vídeo…'**
  String get callWaitingVideo;

  /// No description provided for @callEnded.
  ///
  /// In pt, this message translates to:
  /// **'Chamada encerrada: {reason}'**
  String callEnded(String reason);

  /// No description provided for @pickScreenTitle.
  ///
  /// In pt, this message translates to:
  /// **'O que você quer transmitir?'**
  String get pickScreenTitle;

  /// No description provided for @pickScreenScreens.
  ///
  /// In pt, this message translates to:
  /// **'Telas'**
  String get pickScreenScreens;

  /// No description provided for @pickScreenWindows.
  ///
  /// In pt, this message translates to:
  /// **'Janelas'**
  String get pickScreenWindows;

  /// No description provided for @pickScreenNone.
  ///
  /// In pt, this message translates to:
  /// **'Nenhuma tela encontrada.'**
  String get pickScreenNone;

  /// No description provided for @callPeerChoosing.
  ///
  /// In pt, this message translates to:
  /// **'{name} está escolhendo o que transmitir…'**
  String callPeerChoosing(String name);

  /// No description provided for @callYouShareTo.
  ///
  /// In pt, this message translates to:
  /// **'Você está transmitindo sua tela para {name}'**
  String callYouShareTo(String name);

  /// No description provided for @callEndedPlain.
  ///
  /// In pt, this message translates to:
  /// **'Chamada encerrada'**
  String get callEndedPlain;

  /// No description provided for @metricsTitle.
  ///
  /// In pt, this message translates to:
  /// **'Medição de chamadas'**
  String get metricsTitle;

  /// No description provided for @metricsInfo.
  ///
  /// In pt, this message translates to:
  /// **'{count} conexões registradas neste computador. Os registros não têm IPs, nomes nem conteúdo; só o tipo de caminho, o tempo para conectar e a qualidade. Nada é enviado: exportar gera um arquivo que você decide se envia.'**
  String metricsInfo(int count);

  /// No description provided for @metricsExport.
  ///
  /// In pt, this message translates to:
  /// **'Exportar medições'**
  String get metricsExport;

  /// No description provided for @metricsExported.
  ///
  /// In pt, this message translates to:
  /// **'Arquivo salvo em {path}'**
  String metricsExported(String path);

  /// No description provided for @metricsNone.
  ///
  /// In pt, this message translates to:
  /// **'Nenhuma conexão registrada ainda.'**
  String get metricsNone;
}

class _LDelegate extends LocalizationsDelegate<L> {
  const _LDelegate();

  @override
  Future<L> load(Locale locale) {
    return SynchronousFuture<L>(lookupL(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'pt'].contains(locale.languageCode);

  @override
  bool shouldReload(_LDelegate old) => false;
}

L lookupL(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return LEn();
    case 'pt':
      return LPt();
  }

  throw FlutterError(
    'L.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
